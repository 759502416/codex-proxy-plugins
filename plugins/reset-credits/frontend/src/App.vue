<script setup lang="ts">
import type { AutoSettings, ResetAccount } from '@/api'
import type { ResetPlan } from '@/composables/useResetCredits'
import {
  BaseButton,
  BaseCard,
  BaseCheckbox,
  BaseConfirmModal,
  BaseIconButton,
  BaseSegmented,
  BaseTable,
  BaseTag,
  BaseToast,
  defineTableColumns,
} from '@codex-proxy/ui'
import { FlaskConical, RefreshCw } from '@lucide/vue'
import { computed, onMounted, shallowRef } from 'vue'
import AutoCell from '@/components/AutoCell.vue'
import AutoSettingsModal from '@/components/AutoSettingsModal.vue'
import CreditsCell from '@/components/CreditsCell.vue'
import QuotaWindows from '@/components/QuotaWindows.vue'
import ResultCell from '@/components/ResultCell.vue'
import { useResetCredits } from '@/composables/useResetCredits'
import { statusLabels, statusTags } from '@/utils/presenter'

defineProps<{
  preview: boolean
}>()

type Filter = 'all' | 'limited' | 'available' | 'pending' | 'auto'

type Confirmation
  = | { kind: 'reset', plan: ResetPlan }
    | { kind: 'resume' | 'discard', account: ResetAccount }

const columns = defineTableColumns<ResetAccount>([
  { key: 'selection', kind: 'selection', hideable: false },
  { key: 'identity', label: '账号', kind: 'identity', size: '2xl' },
  { key: 'status', label: '状态', kind: 'status', size: 'sm' },
  { key: 'planTypeDisplay', label: '套餐', kind: 'meta', size: 'sm' },
  { key: 'windows', label: '额度', kind: 'custom', size: 'xl' },
  { key: 'credits', label: '重置卡', kind: 'custom', size: 'lg' },
  { key: 'result', label: '最近结果', kind: 'custom', size: 'xl' },
  { key: 'auto', label: '自动用卡', kind: 'custom', size: 'lg' },
  { key: 'actions', label: '操作', kind: 'actions', size: 'xl' },
])

const { accounts, loading, loadError, credits, operations, working, load, queryMany, prepare, reset, discard, saveAuto } = useResetCredits()
const filter = shallowRef<Filter>('all')
const selectedIds = shallowRef(new Set<string>())
const preparing = shallowRef(false)
const confirmation = shallowRef<Confirmation | null>(null)
const confirmOpen = shallowRef(false)
const autoOpen = shallowRef(false)

const counts = computed(() => ({
  limited: accounts.value.filter(isLimited).length,
  available: accounts.value.filter(hasCredits).length,
  pending: accounts.value.filter(account => account.pending).length,
  auto: accounts.value.filter(account => account.auto).length,
}))
const filterOptions = computed(() => [
  { label: `全部 ${accounts.value.length}`, value: 'all' },
  { label: `额度受限 ${counts.value.limited}`, value: 'limited' },
  { label: `有可用卡 ${counts.value.available}`, value: 'available' },
  { label: `待确认 ${counts.value.pending}`, value: 'pending' },
  { label: `自动用卡 ${counts.value.auto}`, value: 'auto' },
])
const rows = computed(() => accounts.value.filter((account) => {
  switch (filter.value) {
    case 'limited':
      return isLimited(account)
    case 'available':
      return hasCredits(account)
    case 'pending':
      return Boolean(account.pending)
    case 'auto':
      return Boolean(account.auto)
    default:
      return true
  }
}))
const selectedRowKeys = computed(() => [...selectedIds.value])
const selectedAccounts = computed(() => accounts.value.filter(account => selectedIds.value.has(account.id)))
const visibleSelected = computed(() => rows.value.filter(account => selectedIds.value.has(account.id)).length)
const allVisibleSelected = computed(() => rows.value.length > 0 && visibleSelected.value === rows.value.length)
const busy = computed(() => working.value || preparing.value)
// 所选账号设置一致时回填，否则使用默认值
const autoInitial = computed(() => {
  const [first, ...rest] = selectedAccounts.value.map(account => account.auto)
  if (!first || rest.some(item => item?.weeklyThreshold !== first.weeklyThreshold || item?.shortTermThreshold !== first.shortTermThreshold))
    return null
  return first
})
const querying = computed(() => [...credits.values()].some(state => state.state === 'loading'))
const emptyText = computed(() => {
  if (loadError.value)
    return loadError.value
  return filter.value === 'all' ? '没有支持重置卡的 OpenAI 账号' : '没有符合筛选条件的账号'
})

const modal = computed(() => {
  const current = confirmation.value
  if (!current)
    return { title: '', description: '', confirmText: '确认', destructive: false, disabled: true }
  if (current.kind === 'reset') {
    const eligible = current.plan.eligible.length
    return {
      title: '确认重置额度',
      description: `已选 ${current.plan.selected} 个账号，其中 ${eligible} 个有可用重置卡`,
      confirmText: eligible > 0 ? `重置 ${eligible} 个账号` : '确认',
      destructive: false,
      disabled: eligible === 0,
    }
  }
  if (current.kind === 'resume') {
    return {
      title: '确认上次重置',
      description: `${current.account.name} 将使用上次的请求标识重试，上次已扣卡时不会再扣一张`,
      confirmText: '确认上次重置',
      destructive: false,
      disabled: false,
    }
  }
  return {
    title: '放弃待确认的重置',
    description: `${current.account.name} 的上次重置可能已经扣卡，放弃后再重置可能多扣一张`,
    confirmText: '放弃',
    destructive: true,
    disabled: false,
  }
})
const skipped = computed(() => {
  const current = confirmation.value
  if (current?.kind !== 'reset')
    return []
  const plan = current.plan
  return [
    plan.noCredit && `无可用卡 ${plan.noCredit} 个`,
    plan.queryFailed && `查卡失败 ${plan.queryFailed} 个`,
    plan.pending && `待确认 ${plan.pending} 个`,
    plan.unsupported && `不支持消费 ${plan.unsupported} 个`,
  ].filter(Boolean) as string[]
})

function isLimited(account: ResetAccount) {
  return account.status === 'quota_exhausted' || account.status === 'rate_limited'
}

function hasCredits(account: ResetAccount) {
  const state = credits.get(account.id)
  return state?.state === 'ready' && state.summary.availableCount > 0
}

function toggleAll(checked: boolean) {
  const next = new Set(selectedIds.value)
  for (const account of rows.value) {
    if (checked)
      next.add(account.id)
    else
      next.delete(account.id)
  }
  selectedIds.value = next
}

function toggleSelection(id: string) {
  const next = new Set(selectedIds.value)
  if (!next.delete(id))
    next.add(id)
  selectedIds.value = next
}

async function refresh() {
  await load()
  const known = new Set(accounts.value.map(account => account.id))
  selectedIds.value = new Set([...selectedIds.value].filter(id => known.has(id)))
}

async function requestReset(ids: string[]) {
  preparing.value = true
  try {
    confirmation.value = { kind: 'reset', plan: await prepare(ids) }
    confirmOpen.value = true
  }
  finally {
    preparing.value = false
  }
}

function requestAccountAction(kind: 'resume' | 'discard', account: ResetAccount) {
  confirmation.value = { kind, account }
  confirmOpen.value = true
}

async function confirm() {
  const current = confirmation.value
  confirmOpen.value = false
  if (!current)
    return
  if (current.kind === 'reset')
    await reset(current.plan.eligible, false)
  else if (current.kind === 'resume')
    await reset([current.account.id], true)
  else
    await discard(current.account.id)
}

async function applyAuto(settings: AutoSettings | null) {
  if (await saveAuto(selectedAccounts.value.map(account => account.id), settings))
    autoOpen.value = false
}

function clearConfirmation() {
  if (!confirmOpen.value)
    confirmation.value = null
}

onMounted(load)
</script>

<template>
  <main class="flex min-w-0 flex-col gap-4 font-sans text-cp-text" :style="{ minHeight: preview ? '100dvh' : 'inherit' }">
    <div
      v-if="preview"
      class="flex shrink-0 items-center gap-2 rounded-cp bg-cp-warning-container px-3 py-2 text-cp-sm text-cp-warning-on-container"
      role="note"
    >
      <FlaskConical class="size-4 shrink-0" aria-hidden="true" />
      独立模拟预览，不代表宿主安装或集成结果
    </div>

    <BaseCard padding="compact">
      <template #header>
        <div class="flex flex-wrap items-center justify-between gap-3">
          <BaseSegmented v-model="filter" label="账号筛选" size="sm" :options="filterOptions" />
          <div class="flex flex-wrap items-center gap-2">
            <span v-if="selectedIds.size > 0" class="text-cp-sm text-cp-text-tertiary">已选 {{ selectedIds.size }} 个</span>
            <BaseIconButton label="刷新账号列表" :loading="loading" :disabled="busy" @click="refresh">
              <RefreshCw />
            </BaseIconButton>
            <BaseButton
              size="sm"
              :loading="querying"
              :disabled="selectedIds.size === 0 || busy"
              @click="queryMany([...selectedIds])"
            >
              查询重置卡
            </BaseButton>
            <BaseButton
              size="sm"
              :disabled="selectedAccounts.length === 0 || busy"
              @click="autoOpen = true"
            >
              自动用卡
            </BaseButton>
            <BaseButton
              size="sm"
              variant="primary"
              :loading="preparing"
              :disabled="selectedAccounts.length === 0 || busy"
              @click="requestReset(selectedAccounts.map(account => account.id))"
            >
              批量重置
            </BaseButton>
          </div>
        </div>
      </template>

      <template #body>
        <BaseTable
          class="[--cp-table-row-height:72px]"
          :columns="columns"
          :rows="rows"
          :loading="loading"
          :selected-row-keys="selectedRowKeys"
          :empty-text="emptyText"
        >
          <template #header-selection>
            <BaseCheckbox
              :model-value="allVisibleSelected"
              :indeterminate="visibleSelected > 0 && !allVisibleSelected"
              label="选择当前列出的账号"
              @update:model-value="toggleAll"
            />
          </template>

          <template #selection="{ row }">
            <BaseCheckbox
              :model-value="selectedIds.has(row.id)"
              label="选择账号"
              @update:model-value="toggleSelection(row.id)"
            />
          </template>

          <template #identity="{ row }">
            <div class="grid min-w-0 gap-0.5">
              <span class="truncate font-emphasis text-cp-text">{{ row.name }}</span>
              <span v-if="row.email && row.email !== row.name" class="truncate text-cp-xs text-cp-text-tertiary">{{ row.email }}</span>
            </div>
          </template>

          <template #status="{ row }">
            <div class="grid justify-items-center gap-0.5">
              <BaseTag :type="statusTags[row.status]" size="sm">
                {{ statusLabels[row.status] }}
              </BaseTag>
              <span v-if="row.rateLimitRecoveryDisplay" class="text-cp-xs whitespace-nowrap text-cp-text-tertiary">{{ row.rateLimitRecoveryDisplay }}</span>
            </div>
          </template>

          <template #windows="{ row }">
            <QuotaWindows :windows="row.windows" />
          </template>

          <template #credits="{ row }">
            <CreditsCell :credits="credits.get(row.id)" />
          </template>

          <template #result="{ row }">
            <ResultCell :account="row" :operation="operations.get(row.id)" />
          </template>

          <template #auto="{ row }">
            <AutoCell :account="row" />
          </template>

          <template #actions="{ row }">
            <div class="flex items-center gap-1">
              <template v-if="row.pending">
                <BaseButton size="sm" variant="soft" :disabled="busy" @click="requestAccountAction('resume', row)">
                  确认上次重置
                </BaseButton>
                <BaseButton size="sm" variant="ghost" :disabled="busy" @click="requestAccountAction('discard', row)">
                  放弃
                </BaseButton>
              </template>
              <template v-else>
                <BaseButton
                  size="sm"
                  variant="ghost"
                  :loading="credits.get(row.id)?.state === 'loading'"
                  :disabled="busy"
                  @click="queryMany([row.id])"
                >
                  查卡
                </BaseButton>
                <BaseButton
                  size="sm"
                  variant="soft"
                  :loading="operations.get(row.id)?.running"
                  :disabled="!row.consumable || busy"
                  @click="requestReset([row.id])"
                >
                  重置
                </BaseButton>
              </template>
            </div>
          </template>
        </BaseTable>
      </template>
    </BaseCard>

    <AutoSettingsModal
      v-model="autoOpen"
      :count="selectedAccounts.length"
      :saving="working"
      :initial="autoInitial"
      @save="applyAuto"
    />

    <BaseConfirmModal
      v-model="confirmOpen"
      :title="modal.title"
      :description="modal.description"
      :confirm-text="modal.confirmText"
      :destructive="modal.destructive"
      :confirm-disabled="modal.disabled"
      @confirm="confirm"
      @after-leave="clearConfirmation"
    >
      <ul v-if="confirmation?.kind === 'reset'" class="m-0 grid gap-1 pl-5 text-cp-sm text-cp-text-secondary">
        <li>每个账号消耗 1 张最早到期的重置卡，扣卡后不可撤销</li>
        <li>重置后由网关重新读取上游额度并恢复账号状态</li>
        <li v-if="skipped.length > 0">
          跳过：{{ skipped.join('，') }}
        </li>
      </ul>
    </BaseConfirmModal>
  </main>
  <BaseToast />
</template>
