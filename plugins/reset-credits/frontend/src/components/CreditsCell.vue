<script setup lang="ts">
import type { CreditsState } from '@/composables/useResetCredits'

defineProps<{
  credits?: CreditsState
}>()
</script>

<template>
  <span v-if="!credits" class="text-cp-sm text-cp-text-quaternary">未查询</span>
  <span v-else-if="credits.state === 'loading'" class="text-cp-sm text-cp-text-tertiary">查询中</span>
  <span v-else-if="credits.state === 'error'" class="truncate text-cp-sm text-cp-error-text" :title="credits.message">查询失败</span>
  <div v-else class="grid min-w-0 gap-0.5">
    <strong
      class="font-mono text-cp-sm tabular-nums"
      :class="credits.summary.availableCount > 0 ? 'text-cp-text' : 'text-cp-text-quaternary'"
    >
      {{ credits.summary.availableCount }} 张
    </strong>
    <span v-if="credits.summary.earliestExpiresAtDisplay" class="truncate text-cp-xs text-cp-text-tertiary">
      最早 {{ credits.summary.earliestExpiresAtDisplay }} 到期
    </span>
  </div>
</template>
