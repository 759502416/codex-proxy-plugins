<script setup lang="ts">
import type { AutoSettings } from '@/api'
import { BaseButton, BaseModal, BaseNumberInput, BaseSwitch } from '@codex-proxy/ui'
import { computed, shallowRef, watch } from 'vue'

const props = defineProps<{
  count: number
  saving: boolean
  /** 所选账号都已开启且设置相同时用于回填 */
  initial: AutoSettings | null
}>()

const emit = defineEmits<{
  save: [settings: AutoSettings | null]
}>()

const open = defineModel<boolean>({ default: false })
const weeklyThreshold = shallowRef(100)
const shortTermEnabled = shallowRef(false)
const shortTermThreshold = shallowRef(100)

const valid = computed(() =>
  isThreshold(weeklyThreshold.value) && (!shortTermEnabled.value || isThreshold(shortTermThreshold.value)),
)

watch(open, (value) => {
  if (!value)
    return
  weeklyThreshold.value = props.initial?.weeklyThreshold ?? 100
  shortTermEnabled.value = props.initial?.shortTermThreshold != null
  shortTermThreshold.value = props.initial?.shortTermThreshold ?? 100
})

function isThreshold(value: number) {
  return Number.isInteger(value) && value >= 1 && value <= 100
}

function save() {
  if (!valid.value)
    return
  emit('save', {
    weeklyThreshold: weeklyThreshold.value,
    shortTermThreshold: shortTermEnabled.value ? shortTermThreshold.value : null,
  })
}
</script>

<template>
  <BaseModal
    v-model="open"
    title="自动用卡"
    :description="`应用到已选的 ${count} 个账号`"
    size="sm"
    :dismissible="!saving"
  >
    <div class="grid gap-4 text-cp-sm text-cp-text-secondary">
      <div class="flex items-center justify-between gap-3">
        <span class="font-emphasis text-cp-text">周额度达到</span>
        <BaseNumberInput v-model="weeklyThreshold" label="周额度阈值" :min="1" :max="100" unit="%" />
      </div>
      <div class="flex items-center justify-between gap-3">
        <BaseSwitch v-model="shortTermEnabled" label="5 小时额度也触发" show-label />
        <BaseNumberInput
          v-model="shortTermThreshold"
          label="5 小时额度阈值"
          :min="1"
          :max="100"
          unit="%"
          :disabled="!shortTermEnabled"
        />
      </div>
      <ul class="m-0 grid gap-1 pl-5">
        <li>每个周额度周期最多自动用一张最早到期的卡</li>
        <li>扣卡不可撤销，5 小时额度通常会自行恢复</li>
      </ul>
    </div>

    <template #footer>
      <BaseButton variant="ghost" :disabled="saving" @click="emit('save', null)">
        关闭自动用卡
      </BaseButton>
      <BaseButton variant="primary" :loading="saving" :disabled="!valid" @click="save">
        保存
      </BaseButton>
    </template>
  </BaseModal>
</template>
