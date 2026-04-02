"use client"

import * as React from 'react'
import { useTheme } from '@/components/theme-provider'

interface LogoProps {
  className?: string
}

const ARGUS_DARK_SRC = new URL("./argus-dark.svg", import.meta.url).href
const ARGUS_LIGHT_SRC = new URL("./argus-light.svg", import.meta.url).href

const Logo = ({ className }: LogoProps) => {
  const { resolvedTheme } = useTheme()
  const [mounted, setMounted] = React.useState(false)

  React.useEffect(() => {
    setMounted(true)
  }, [])

  // During SSR/static generation, render light logo as default
  // After mount, use the actual theme
  const src = mounted && resolvedTheme === 'dark' ? ARGUS_DARK_SRC : ARGUS_LIGHT_SRC

  return (
    <img
      src={src}
      alt="ArgusWing"
      className={className}
      width={32}
      height={32}
    />
  )
}

export default Logo
