"use client"

export default function SettingsLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <div className="mx-auto w-full max-w-7xl px-6 py-4">
        {children}
      </div>
    </div>
  )
}
