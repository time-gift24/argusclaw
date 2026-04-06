export function normalizeBreadcrumbs(breadcrumbs) {
  return breadcrumbs.map((b) => {
    if (typeof b === 'string') return { label: b }
    return b
  })
}
