<script setup>
import { computed, useSlots } from 'vue'
import { normalizeBreadcrumbs } from '../utils/pageShell'

const props = defineProps({
  title: { type: String, required: true },
  description: { type: String, default: '' },
  breadcrumbs: { type: Array, default: () => [] },
  widthClass: { type: String, default: 'max-w-7xl' },
  contentClass: { type: String, default: 'space-y-4' },
})

const slots = useSlots()
const normalizedBreadcrumbs = computed(() => normalizeBreadcrumbs(props.breadcrumbs))
const hasActions = computed(() => Boolean(slots.actions))
const hasHeader = computed(() => normalizedBreadcrumbs.value.length || hasActions.value)
</script>

<template>
  <div>
    <header v-if="hasHeader" class="mb-4 animate-fade-up">
      <div class="mx-auto max-w-7xl">
        <div class="min-h-5 flex items-center">
          <nav
            v-if="normalizedBreadcrumbs.length"
            class="flex min-w-0 flex-wrap items-center gap-2 text-xs font-semibold text-on-surface-variant"
            aria-label="Breadcrumb"
          >
            <template v-for="(crumb, index) in normalizedBreadcrumbs" :key="`${crumb.label}-${index}`">
              <router-link
                v-if="crumb.to && index < normalizedBreadcrumbs.length - 1"
                :to="crumb.to"
                class="transition-colors hover:text-primary"
              >
                {{ crumb.label }}
              </router-link>
              <span v-else class="text-on-surface" :class="index < normalizedBreadcrumbs.length - 1 ? 'font-medium' : 'font-bold'">
                {{ crumb.label }}
              </span>
              <svg
                v-if="index < normalizedBreadcrumbs.length - 1"
                class="h-3.5 w-3.5 text-on-surface-variant/70"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
              >
                <polyline points="9 18 15 12 9 6" />
              </svg>
            </template>
          </nav>
        </div>
        <div v-if="hasActions" class="mt-3 flex items-center gap-2">
          <slot name="actions" />
        </div>
      </div>
      <h1 class="sr-only">{{ title }}</h1>
    </header>
    <div :class="[widthClass, contentClass, 'mx-auto']">
      <slot />
    </div>
  </div>
</template>
