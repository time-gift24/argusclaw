<script setup lang="ts">
import { computed } from "vue";
import { useRoute } from "vue-router";
import { TinyBreadcrumb, TinyBreadcrumbItem } from "@/lib/opentiny";
import { adminNavItems } from "@/app/nav";

const route = useRoute();

const breadcrumbs = computed(() => {
  const matched = route.matched;
  const crumbs = [];

  // Home/Root entry if not already there
  if (matched.length > 0 && matched[0].path !== "/") {
    crumbs.push({
      label: "管理中心",
      to: { path: "/" },
    });
  }

  for (const record of matched) {
    const label = record.meta?.breadcrumb;
    if (label) {
      const resolvedLabel = typeof label === 'function'
        ? label(route)
        : label;

      crumbs.push({
        label: resolvedLabel,
        to: { path: record.path === "" ? "/" : record.path },
      });
    } else {
      // Try to find in nav items if it's a top-level route
      const navItem = adminNavItems.find(item => item.to === record.path);
      if (navItem) {
        crumbs.push({
          label: navItem.label,
          to: { path: navItem.to },
        });
      }
    }
  }

  // Filter out duplicates (sometimes / matches both root and children)
  return crumbs.filter((crumb, index, self) =>
    index === self.findIndex((t) => t.label === crumb.label)
  );
});
</script>

<template>
  <nav class="app-breadcrumb" aria-label="Breadcrumb">
    <TinyBreadcrumb>
      <TinyBreadcrumbItem v-for="(crumb, index) in breadcrumbs" :key="index" :to="crumb.to">
        {{ crumb.label }}
      </TinyBreadcrumbItem>
    </TinyBreadcrumb>
  </nav>
</template>

<style scoped>
.app-breadcrumb {
  margin-bottom: var(--space-2);
}

:deep(.tiny-breadcrumb__item) {
  font-size: var(--text-xs);
}

:deep(.tiny-breadcrumb__inner) {
  color: var(--text-muted);
}

:deep(.tiny-breadcrumb__item:last-child .tiny-breadcrumb__inner) {
  color: var(--accent);
  font-weight: 590;
}
</style>
