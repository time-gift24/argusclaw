"use client"

export default function SettingsLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return (
    <div className="mx-auto w-full max-w-7xl px-6 py-4">
      {children}
    </div>
  )
}
