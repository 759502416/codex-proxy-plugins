// @env browser
import type { PluginHost } from './types'
import { isRecord } from '@/utils/guards'

export class ApiError extends Error {
  constructor(message: string, readonly status: number, readonly code: string | null) {
    super(message)
    this.name = 'ApiError'
  }
}

function getHost(): PluginHost {
  const host = window.codexProxyPlugin
  if (!host || host.version !== 2)
    throw new Error('请从网关的插件管理页面打开')
  return host
}

// url 是插件注册的相对路由；会话与实例定位由宿主负责，不直接访问管理端 HTTP 接口
export default async function request(url: string, method: 'GET' | 'POST', data?: unknown): Promise<unknown> {
  const reply = await getHost().request({
    path: url,
    method,
    ...(data === undefined ? {} : { contentType: 'application/json', body: JSON.stringify(data) }),
  })
  if (reply.contentType.split(';', 1)[0]?.trim().toLowerCase() !== 'application/json')
    throw new ApiError('插件返回了不支持的内容类型', reply.status, null)

  let value: unknown
  try {
    value = JSON.parse(new TextDecoder().decode(reply.body))
  }
  catch {
    throw new ApiError('插件返回了无效 JSON', reply.status, null)
  }
  if (reply.status < 200 || reply.status >= 300) {
    const error = isRecord(value) && isRecord(value.error) ? value.error : {}
    throw new ApiError(
      typeof error.message === 'string' ? error.message : `插件请求失败（HTTP ${reply.status}）`,
      reply.status,
      typeof error.code === 'string' ? error.code : null,
    )
  }
  return value
}
