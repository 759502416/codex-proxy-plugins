<script setup lang="ts">
import type { ResetAccount } from '@/api'
import { BaseTag } from '@codex-proxy/ui'
import { computed } from 'vue'
import { autoResultPresentation, autoSettingsLabel } from '@/utils/presenter'

const props = defineProps<{
  account: ResetAccount
}>()

const last = computed(() => props.account.autoLast ? autoResultPresentation(props.account.autoLast, Date.now()) : null)
</script>

<template>
  <div class="grid min-w-0 justify-items-start gap-1">
    <span v-if="account.auto" class="font-mono text-cp-sm tabular-nums text-cp-text">{{ autoSettingsLabel(account.auto) }}</span>
    <span v-else class="text-cp-sm text-cp-text-quaternary">关闭</span>
    <template v-if="last">
      <BaseTag :type="last.type" size="sm">
        {{ last.label }}
      </BaseTag>
      <span class="max-w-full truncate text-cp-xs text-cp-text-tertiary" :title="last.detail">{{ last.detail }}</span>
    </template>
  </div>
</template>
