// @env browser
import type { PluginHost, ResetAccount } from '../api'
import {
  applyResolvedTheme,
  DEFAULT_CUSTOM_THEME_COLOR,
  DEFAULT_THEME_COLOR,
  resolveTheme,
} from '@codex-proxy/ui/theme'

// 独立预览只模拟插件管理路由，用于检查布局和交互，不访问宿主或上游
const encoder = new TextEncoder()

function window5h(used: number | null) {
  return {
    key: 'codex-5h',
    label: '5小时额度',
    usedPercent: used,
    usedPercentDisplay: used === null ? '—' : `${used}%`,
    resetAtDisplay: '10-08 21:30',
    limitReached: used !== null && used >= 100,
  }
}

function weekly(used: number) {
  return {
    key: 'codex-7d',
    label: '周额度',
    usedPercent: used,
    usedPercentDisplay: `${used}%`,
    resetAtDisplay: '10-13 08:00',
    limitReached: used >= 100,
  }
}

let accounts: ResetAccount[] = []
const inventory = new Map<string, number>()

// 只在开发模式下生成模拟数据，生产构建会把它作为死代码移除
function seed() {
  accounts = [
    ['acct-plus-1', 'plus-main', 'plus-main@example.com', 'Plus', 'quota_exhausted', 100, 100],
    ['acct-plus-2', 'plus-backup', 'plus-backup@example.com', 'Plus', 'rate_limited', 82, 100],
    ['acct-pro-1', 'pro-team', 'pro-team@example.com', 'Pro', 'normal', 36, 64],
    ['acct-team-1', 'team-seat', null, 'Team', 'normal', 12, 21],
    ['acct-free-1', 'free-trial', 'free-trial@example.com', 'Free', 'quota_exhausted', null, 100],
  ].map(([id, name, email, plan, status, short, week]) => ({
    id: id as string,
    name: name as string,
    email: email as string | null,
    planTypeDisplay: plan as string,
    status: status as ResetAccount['status'],
    enabled: true,
    consumable: true,
    quotaRefreshedAtDisplay: '10-08 18:20',
    rateLimitRecoveryDisplay: status === 'rate_limited' ? '19:05 恢复' : null,
    windows: [window5h(short as number | null), weekly(week as number)],
    pending: null,
    last: null,
    auto: null,
    autoLast: null,
  }))
  accounts[1]!.last = { outcome: 'reset', code: 'reset', atMs: Date.now() - 86_400_000 }
  accounts[1]!.auto = { weeklyThreshold: 100, shortTermThreshold: null }
  accounts[1]!.autoLast = { outcome: 'reset', code: 'reset', trigger: 'weekly', atMs: Date.now() - 86_400_000 }
  accounts[3]!.auto = { weeklyThreshold: 95, shortTermThreshold: 100 }
  accounts[3]!.autoLast = { outcome: 'no_credit', code: null, trigger: 'short_term', atMs: Date.now() - 3_600_000 }
  for (const [id, count] of [['acct-plus-1', 2], ['acct-plus-2', 1], ['acct-pro-1', 3], ['acct-team-1', 0]] as const)
    inventory.set(id, count)
}

function json(value: unknown, status = 200) {
  return Promise.resolve({ status, contentType: 'application/json', body: encoder.encode(JSON.stringify(value)).buffer })
}

function error(code: string, message: string, status: number) {
  return json({ error: { code, message } }, status)
}

function credits(id: string) {
  const count = inventory.get(id) ?? 0
  return { availableCount: count, earliestExpiresAtDisplay: count > 0 ? '2026-10-31 12:00' : null }
}

async function managementRequest(input: Parameters<PluginHost['request']>[0]) {
  await new Promise(resolve => window.setTimeout(resolve, 300))
  const body = input.body ? JSON.parse(input.body) as Record<string, unknown> : {}
  const account = accounts.find(item => item.id === body.accountId)
  if (input.method === 'GET' && input.path === 'api/accounts')
    return json({ accounts })
  if (input.path === 'api/auto') {
    const ids = new Set(body.accountIds as string[])
    for (const item of accounts) {
      if (ids.has(item.id))
        item.auto = body.settings as ResetAccount['auto']
    }
    return json({ updated: ids.size })
  }
  if (!account)
    return error('invalid_request', '账号 ID 无效', 400)
  if (input.path === 'api/credits') {
    if (account.id === 'acct-free-1')
      return error('host_rejected', '上游服务拒绝了重置卡查询', 502)
    return json(credits(account.id))
  }
  if (input.path === 'api/discard') {
    account.pending = null
    account.last = { outcome: 'discarded', code: null, atMs: Date.now() }
    return json({ discarded: true })
  }
  if (input.path === 'api/reset') {
    if (!body.resume && account.pending)
      return error('pending_exists', '该账号上次重置的结果尚未确认，请先确认上次重置', 409)
    // 预览中备用账号首次重置模拟结果未知，便于检查待确认流程
    if (account.id === 'acct-plus-2' && !body.resume) {
      inventory.set(account.id, (inventory.get(account.id) ?? 1) - 1)
      account.pending = { startedAtMs: Date.now() }
      return json({ outcome: 'unknown', message: '消费结果暂不确定，确认上次重置时会复用同一个请求标识', warnings: [], credits: null, account: null })
    }
    const count = inventory.get(account.id) ?? 0
    if (!body.resume && count === 0)
      return json({ outcome: 'no_credit', message: '当前没有可用的主动重置卡', warnings: [], credits: credits(account.id), account: null })
    if (!body.resume)
      inventory.set(account.id, count - 1)
    account.pending = null
    account.status = 'normal'
    account.rateLimitRecoveryDisplay = null
    account.windows = [window5h(0), weekly(0)]
    account.last = { outcome: 'reset', code: 'reset', atMs: Date.now() }
    return json({
      outcome: 'reset',
      message: body.resume ? '上次重置已完成' : '额度已重置',
      warnings: [],
      credits: credits(account.id),
      account,
    })
  }
  return error('not_found', '未找到插件管理接口', 404)
}

export function installPreviewHost(): boolean {
  if (!import.meta.env.DEV)
    return false
  seed()

  const parameters = new URLSearchParams(window.location.search)
  const theme = parameters.get('theme') === 'dark' ? 'dark' : 'light'
  applyResolvedTheme(
    document.documentElement,
    resolveTheme(theme, DEFAULT_THEME_COLOR, DEFAULT_CUSTOM_THEME_COLOR),
  )
  const value: PluginHost = { version: 2, theme, request: managementRequest }
  Object.defineProperty(window, 'codexProxyPlugin', { configurable: true, value })
  return true
}
