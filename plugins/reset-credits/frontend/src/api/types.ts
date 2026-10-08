export interface ManagementReply {
  status: number
  contentType: string
  body: ArrayBuffer
}

export interface PluginHost {
  readonly version: 2
  readonly theme: 'light' | 'dark'
  request: (input: {
    method: 'GET' | 'POST'
    path: string
    query?: string
    contentType?: string
    body?: string
  }) => Promise<ManagementReply>
}

export type AccountStatus = 'normal' | 'quota_exhausted' | 'rate_limited' | 'disabled' | 'error'

export interface QuotaWindow {
  key: string
  label: string
  usedPercent: number | null
  usedPercentDisplay: string
  resetAtDisplay: string
  limitReached: boolean
}

export type LastOutcome = 'reset' | 'no_credit' | 'not_executed' | 'failed' | 'discarded'

export interface LastResult {
  outcome: LastOutcome
  code: string | null
  atMs: number
}

export interface ResetAccount {
  id: string
  name: string
  email: string | null
  planTypeDisplay: string
  status: AccountStatus
  enabled: boolean
  consumable: boolean
  quotaRefreshedAtDisplay: string
  rateLimitRecoveryDisplay: string | null
  windows: QuotaWindow[]
  /** 已发出但未确认结果的消费；存在时不能发起新的重置 */
  pending: { startedAtMs: number } | null
  last: LastResult | null
  /** 为空表示未开启自动用卡 */
  auto: AutoSettings | null
  autoLast: AutoResult | null
}

export interface AutoSettings {
  /** 周额度已用百分比达到该值时用卡 */
  weeklyThreshold: number
  /** 为空表示不因 5 小时额度用卡 */
  shortTermThreshold: number | null
}

export type AutoOutcome = 'resetting' | 'reset' | 'no_credit' | 'not_executed' | 'failed' | 'unknown' | 'cycle_used'

export type AutoTrigger = 'weekly' | 'short_term' | 'both'

export interface AutoResult {
  outcome: AutoOutcome
  code: string | null
  trigger: AutoTrigger | null
  atMs: number
}

export interface CreditsSummary {
  availableCount: number
  earliestExpiresAtDisplay: string | null
}

export type ResetOutcome = 'reset' | 'no_credit' | 'not_executed' | 'failed' | 'unknown'

export interface ResetResult {
  outcome: ResetOutcome
  message: string
  warnings: string[]
  credits: CreditsSummary | null
  account: ResetAccount | null
}
