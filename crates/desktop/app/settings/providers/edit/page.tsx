'use client'

import { Suspense, useMemo } from "react"
import { useSearchParams } from "next/navigation"
import { ProviderEditor } from "@/components/settings"

function EditProviderContent() {
  const searchParams = useSearchParams()
  const providerId = useMemo(() => {
    const id = searchParams.get("id")
    return id ? parseInt(id, 10) : undefined
  }, [searchParams])

  return <ProviderEditor providerId={providerId} />
}

export default function EditProviderPage() {
  return (
    <Suspense
      fallback={
        <div className="flex items-center justify-center h-64">
          <div className="text-muted-foreground">加载中...</div>
        </div>
      }
    >
      <EditProviderContent />
    </Suspense>
  )
}
