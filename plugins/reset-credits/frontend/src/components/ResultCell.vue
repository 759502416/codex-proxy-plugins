<script setup lang="ts">
import type { ResetAccount } from '@/api'
import type { OperationState } from '@/composables/useResetCredits'
import { BaseTag } from '@codex-proxy/ui'
import { computed } from 'vue'
import { formatDateTime, outcomeLabels, outcomeTags } from '@/utils/presenter'

const props = defineProps<{
  account: ResetAccount
  operation?: OperationState
}>()

// 本次页面内的结果优先；待确认记录来自插件状态，刷新后仍然保留
const detail = computed(() => {
  const operation = props.operation
  if (operation?.running)
    return null
  if (props.account.pending && operation?.outcome !== 'failed') {
    return {
      label: '待确认',
      type: 'warning' as const,
      text: `${formatDateTime(props.account.pending.startedAtMs)} 发起，结果未确认`,
      warnings: [],
    }
  }
  if (operation?.outcome) {
    return {
      label: outcomeLabels[operation.outcome],
      type: outcomeTags[operation.outcome],
      text: operation.message ?? '',
      warnings: operation.warnings ?? [],
    }
  }
  const last = props.account.last
  if (last) {
    return {
      label: outcomeLabels[last.outcome],
      type: outcomeTags[last.outcome],
      text: formatDateTime(last.atMs),
      warnings: [],
    }
  }
  return null
})
</script>

<template>
  <span v-if="operation?.running" class="text-cp-sm text-cp-text-tertiary">处理中</span>
  <div v-else-if="detail" class="grid min-w-0 justify-items-start gap-1">
    <BaseTag :type="detail.type" size="sm">
      {{ detail.label }}
    </BaseTag>
    <span v-if="detail.text" class="max-w-full truncate text-cp-xs text-cp-text-tertiary" :title="detail.text">{{ detail.text }}</span>
    <span
      v-for="warning in detail.warnings"
      :key="warning"
      class="max-w-full truncate text-cp-xs text-cp-warning-text"
      :title="warning"
    >
      {{ warning }}
    </span>
  </div>
  <span v-else class="text-cp-sm text-cp-text-quaternary">—</span>
</template>
