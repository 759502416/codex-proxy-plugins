<script setup lang="ts">
import type { QuotaWindow } from '@/api'
import { usageTone } from '@/utils/presenter'

defineProps<{
  windows: QuotaWindow[]
}>()

function barWidth(window: QuotaWindow) {
  if (window.usedPercent === null)
    return '0'
  return `${Math.min(Math.max(window.usedPercent, 0), 100)}%`
}
</script>

<template>
  <div v-if="windows.length > 0" class="grid w-full min-w-0 gap-1.5 py-1">
    <div
      v-for="window in windows"
      :key="window.key"
      class="grid min-w-0 gap-1"
      :title="`${window.label} ${window.usedPercentDisplay}，${window.resetAtDisplay} 重置`"
    >
      <div class="flex min-w-0 items-baseline justify-between gap-2 text-cp-xs leading-[14px] font-bold">
        <span class="truncate text-cp-text-quaternary">{{ window.label }}</span>
        <span class="shrink-0 font-mono tabular-nums" :class="usageTone(window.usedPercent).text">{{ window.usedPercentDisplay }}</span>
      </div>
      <div class="h-[3px] w-full overflow-hidden rounded-full bg-cp-fill-tertiary" aria-hidden="true">
        <div class="h-full rounded-full" :class="usageTone(window.usedPercent).bar" :style="{ width: barWidth(window) }" />
      </div>
    </div>
  </div>
  <span v-else class="text-cp-sm text-cp-text-quaternary">暂无额度数据</span>
</template>
