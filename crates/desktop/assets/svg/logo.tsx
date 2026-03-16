"use client"

import * as React from 'react'
import { useTheme } from 'next-themes'
import Image from 'next/image'

import ArgusDark from './argus-dark.svg'
import ArgusLight from './argus-light.svg'

interface LogoProps {
  className?: string
}

const Logo = ({ className }: LogoProps) => {
  const { resolvedTheme } = useTheme()
  const [mounted, setMounted] = React.useState(false)

  React.useEffect(() => {
    setMounted(true)
  }, [])

  // During SSR/static generation, render light logo as default
  // After mount, use the actual theme
  const src = mounted && resolvedTheme === 'dark' ? ArgusDark : ArgusLight

  return (
    <Image
      src={src}
      alt="ArgusClaw"
      className={className}
      width={32}
      height={32}
    />
  )
}

export default Logo
