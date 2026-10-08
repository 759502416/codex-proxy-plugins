import type { AccountStatus, AutoOutcome, AutoResult, AutoSettings, AutoTrigger, LastOutcome, ResetOutcome } from '@/api'

type TagType = 'neutral' | 'primary' | 'success' | 'info' | 'warning' | 'danger'

// 与宿主账号页的状态文案和色调保持一致
export const statusLabels: Record<AccountStatus, string> = {
  normal: '正常',
  quota_exhausted: '配额耗尽',
  rate_limited: '限流中',
  disabled: '已停用',
  error: '错误',
}

export const statusTags: Record<AccountStatus, TagType> = {
  normal: 'success',
  quota_exhausted: 'warning',
  rate_limited: 'warning',
  disabled: 'neutral',
  error: 'danger',
}

export const outcomeLabels: Record<ResetOutcome | LastOutcome, string> = {
  reset: '已重置',
  no_credit: '无可用卡',
  not_executed: '上游未执行',
  failed: '失败',
  unknown: '结果未知',
  discarded: '已放弃',
}

export const outcomeTags: Record<ResetOutcome | LastOutcome, TagType> = {
  reset: 'success',
  no_credit: 'neutral',
  not_executed: 'warning',
  failed: 'danger',
  unknown: 'warning',
  discarded: 'neutral',
}

/** 超过该时长仍处于消费中，说明上次处理被中断 */
const RESETTING_STALE_MS = 2 * 60_000

const autoOutcomeLabels: Record<AutoOutcome, string> = {
  resetting: '处理中',
  reset: '已用卡',
  no_credit: '无可用卡',
  not_executed: '上游未执行',
  failed: '失败',
  unknown: '结果未知',
  cycle_used: '本周期已用卡',
}

const autoOutcomeTags: Record<AutoOutcome, TagType> = {
  resetting: 'info',
  reset: 'success',
  no_credit: 'neutral',
  not_executed: 'warning',
  failed: 'danger',
  unknown: 'warning',
  cycle_used: 'neutral',
}

const autoCodeLabels: Record<string, string> = {
  quota_refresh_failed: '额度刷新失败',
  weekly_reset_unknown: '缺少周额度重置时间',
  credits_query_failed: '查卡失败',
  account_busy: '账号有其他重置操作',
  state_unavailable: '插件记录读写失败',
}

const triggerLabels: Record<AutoTrigger, string> = {
  weekly: '周额度触发',
  short_term: '5 小时额度触发',
  both: '周与 5 小时额度触发',
}

export function autoSettingsLabel(settings: AutoSettings) {
  const weekly = `周 ${settings.weeklyThreshold}%`
  return settings.shortTermThreshold === null ? weekly : `${weekly} · 5h ${settings.shortTermThreshold}%`
}

export function autoResultPresentation(result: AutoResult, now: number) {
  const interrupted = result.outcome === 'resetting' && now - result.atMs > RESETTING_STALE_MS
  const detail = [
    formatDateTime(result.atMs),
    result.trigger ? triggerLabels[result.trigger] : null,
    result.outcome === 'failed' && result.code ? (autoCodeLabels[result.code] ?? result.code) : null,
  ].filter(Boolean).join(' · ')
  return {
    label: interrupted ? '已中断' : autoOutcomeLabels[result.outcome],
    type: interrupted ? 'warning' as const : autoOutcomeTags[result.outcome],
    detail,
  }
}

/** 与宿主额度窗口的阈值一致：95% 以上告急，80% 以上警告 */
export function usageTone(usedPercent: number | null) {
  if (usedPercent === null)
    return { bar: 'bg-cp-border', text: 'text-cp-text-quaternary' }
  if (usedPercent >= 95)
    return { bar: 'bg-cp-error', text: 'text-cp-error-text' }
  if (usedPercent >= 80)
    return { bar: 'bg-cp-warning', text: 'text-cp-warning-text' }
  return { bar: 'bg-cp-success', text: 'text-cp-success-text' }
}

export function formatDateTime(value: number): string {
  return new Intl.DateTimeFormat('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  }).format(new Date(value))
}
