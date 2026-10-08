import type { AutoSettings, CreditsSummary, ResetAccount, ResetOutcome } from '@/api'
import { toast } from '@codex-proxy/ui'
import { shallowReactive, shallowRef } from 'vue'
import { ApiError, discardPending, listAccounts, queryCredits, resetAccount, updateAutoSettings } from '@/api'

export type CreditsState
  = | { state: 'loading' }
    | { state: 'ready', summary: CreditsSummary, at: number }
    | { state: 'error', message: string }

export interface OperationState {
  running: boolean
  outcome?: ResetOutcome
  message?: string
  warnings?: string[]
}

export interface ResetPlan {
  selected: number
  eligible: string[]
  noCredit: number
  queryFailed: number
  pending: number
  unsupported: number
}

/** 确认弹窗使用的卡数在此时间内可复用；插件扣卡前仍会重新查询 */
const CREDITS_FRESH_MS = 2 * 60_000
const QUERY_CONCURRENCY = 4
const RESET_CONCURRENCY = 3

async function runPool<T>(items: T[], limit: number, task: (item: T) => Promise<void>) {
  let next = 0
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, async () => {
    while (next < items.length)
      await task(items[next++]!)
  }))
}

function errorMessage(error: unknown, fallback: string) {
  return error instanceof Error && error.message ? error.message : fallback
}

export function useResetCredits() {
  const accounts = shallowRef<ResetAccount[]>([])
  const loading = shallowRef(false)
  const loadError = shallowRef('')
  const credits = shallowReactive(new Map<string, CreditsState>())
  const operations = shallowReactive(new Map<string, OperationState>())
  const working = shallowRef(false)

  async function load() {
    loading.value = true
    loadError.value = ''
    try {
      accounts.value = await listAccounts()
    }
    catch (error) {
      loadError.value = errorMessage(error, '账号列表加载失败')
    }
    finally {
      loading.value = false
    }
  }

  async function queryMany(ids: string[], onlyStale = false) {
    const now = Date.now()
    const targets = ids.filter((id) => {
      const current = credits.get(id)
      if (current?.state === 'loading')
        return false
      return !onlyStale || current?.state !== 'ready' || now - current.at > CREDITS_FRESH_MS
    })
    await runPool(targets, QUERY_CONCURRENCY, async (id) => {
      credits.set(id, { state: 'loading' })
      try {
        credits.set(id, { state: 'ready', summary: await queryCredits(id), at: Date.now() })
      }
      catch (error) {
        credits.set(id, { state: 'error', message: errorMessage(error, '查询失败') })
      }
    })
  }

  /** 先补齐卡数，再按账号能力、待确认记录和剩余张数筛出可重置账号 */
  async function prepare(ids: string[]): Promise<ResetPlan> {
    await queryMany(ids, true)
    const plan: ResetPlan = { selected: ids.length, eligible: [], noCredit: 0, queryFailed: 0, pending: 0, unsupported: 0 }
    const byId = new Map(accounts.value.map(account => [account.id, account]))
    for (const id of ids) {
      const account = byId.get(id)
      const current = credits.get(id)
      if (!account?.consumable)
        plan.unsupported += 1
      else if (account.pending)
        plan.pending += 1
      else if (current?.state !== 'ready')
        plan.queryFailed += 1
      else if (current.summary.availableCount <= 0)
        plan.noCredit += 1
      else
        plan.eligible.push(id)
    }
    return plan
  }

  async function reset(ids: string[], resume: boolean) {
    working.value = true
    const counts: Record<ResetOutcome, number> = { reset: 0, no_credit: 0, not_executed: 0, failed: 0, unknown: 0 }
    let withWarnings = 0
    try {
      await runPool(ids, RESET_CONCURRENCY, async (id) => {
        operations.set(id, { running: true })
        try {
          const result = await resetAccount(id, resume)
          counts[result.outcome] += 1
          if (result.warnings.length > 0)
            withWarnings += 1
          operations.set(id, { running: false, ...result })
          // 可能已扣卡但没有回读到新卡数时，旧卡数不再可信
          if (result.credits)
            credits.set(id, { state: 'ready', summary: result.credits, at: Date.now() })
          else if (result.outcome === 'reset' || result.outcome === 'unknown')
            credits.delete(id)
        }
        catch (error) {
          // 插件自身返回的错误都发生在扣卡之前；桥接中断时插件可能已经发出消费，以刷新后的待确认记录为准
          const definite = error instanceof ApiError && error.code !== null
          const outcome = definite ? 'failed' : 'unknown'
          counts[outcome] += 1
          if (!definite)
            credits.delete(id)
          operations.set(id, {
            running: false,
            outcome,
            message: definite ? errorMessage(error, '重置失败') : '请求中断，刷新后确认结果',
          })
        }
      })
      await load()
    }
    finally {
      working.value = false
    }
    report(counts, withWarnings)
  }

  async function discard(id: string) {
    working.value = true
    try {
      await discardPending(id)
      operations.delete(id)
      toast.success('已放弃待确认的重置')
      await load()
    }
    catch (error) {
      toast.error(errorMessage(error, '操作失败'))
    }
    finally {
      working.value = false
    }
  }

  async function saveAuto(ids: string[], settings: AutoSettings | null): Promise<boolean> {
    working.value = true
    try {
      const updated = await updateAutoSettings(ids, settings)
      toast.success(settings ? `已为 ${updated} 个账号开启自动用卡` : `已关闭 ${updated} 个账号的自动用卡`)
      await load()
      return true
    }
    catch (error) {
      toast.error(errorMessage(error, '自动用卡设置保存失败'))
      return false
    }
    finally {
      working.value = false
    }
  }

  return { accounts, loading, loadError, credits, operations, working, load, queryMany, prepare, reset, discard, saveAuto }
}

function report(counts: Record<ResetOutcome, number>, withWarnings: number) {
  const parts = [
    counts.reset && `已重置 ${counts.reset} 个`,
    counts.no_credit && `无可用卡 ${counts.no_credit} 个`,
    counts.not_executed && `上游未执行 ${counts.not_executed} 个`,
    counts.failed && `失败 ${counts.failed} 个`,
    counts.unknown && `结果未知 ${counts.unknown} 个`,
  ].filter(Boolean).join('，')
  if (counts.unknown > 0 || withWarnings > 0) {
    const hint = counts.unknown > 0 ? '结果未知的账号请用“确认上次重置”处理' : '部分账号重置后回读失败，不要重复重置'
    toast.warning(`${parts}，${hint}`, { duration: 8000 })
  }
  else if (counts.failed > 0 || counts.not_executed > 0) {
    toast.warning(parts, { duration: 6000 })
  }
  else {
    toast.success(parts)
  }
}
