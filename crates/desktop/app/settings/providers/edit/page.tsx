'use client'

import { useMemo } from "react"
import { useSearchParams } from "react-router-dom"
import { ProviderEditor } from "@/components/settings"

export default function EditProviderPage() {
  const [searchParams] = useSearchParams()
  const providerId = useMemo(() => {
    const id = searchParams.get("id")
    return id ? parseInt(id, 10) : undefined
  }, [searchParams])

  return <ProviderEditor providerId={providerId} />
}
