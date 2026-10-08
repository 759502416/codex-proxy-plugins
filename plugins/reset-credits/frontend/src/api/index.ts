import type { AutoSettings, CreditsSummary, ResetAccount, ResetResult } from './types'
import { isRecord } from '@/utils/guards'
import request from './request'

export { ApiError } from './request'
export type * from './types'

function field<T>(value: unknown, key: string, check: (item: unknown) => boolean): T {
  if (!isRecord(value) || !check(value[key]))
    throw new Error('插件返回的数据格式无效')
  return value[key] as T
}

export async function listAccounts(): Promise<ResetAccount[]> {
  return field<ResetAccount[]>(await request('api/accounts', 'GET'), 'accounts', Array.isArray)
}

export async function queryCredits(accountId: string): Promise<CreditsSummary> {
  const reply = await request('api/credits', 'POST', { accountId })
  field<number>(reply, 'availableCount', Number.isSafeInteger)
  return reply as CreditsSummary
}

/** resume 为 true 时确认上次未决的消费，插件原样复用上次的请求标识 */
export async function resetAccount(accountId: string, resume: boolean): Promise<ResetResult> {
  const reply = await request('api/reset', 'POST', { accountId, resume })
  field<string>(reply, 'outcome', item => typeof item === 'string')
  field<string[]>(reply, 'warnings', Array.isArray)
  return reply as ResetResult
}

export async function discardPending(accountId: string): Promise<void> {
  await request('api/discard', 'POST', { accountId })
}

/** settings 为空时关闭所选账号的自动用卡 */
export async function updateAutoSettings(accountIds: string[], settings: AutoSettings | null): Promise<number> {
  return field<number>(await request('api/auto', 'POST', { accountIds, settings }), 'updated', Number.isSafeInteger)
}
