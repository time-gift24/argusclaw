'use client'

// React Imports
import type { ImgHTMLAttributes } from 'react'

const Logo = (props: ImgHTMLAttributes<HTMLImageElement>) => {
  return <img src='/argusx.svg' width='1em' height='1em' {...props} />
}

export default Logo
