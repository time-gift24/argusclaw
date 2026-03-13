import Navbar from '@/components/shadcn-studio/blocks/navbar-component-09/navbar-component-09'

const navigationItems = [
  {
    title: 'Home',
    href: '#'
  },
  {
    title: 'Products',
    href: '#',
    isActive: true
  },
  {
    title: 'About us',
    href: '#'
  },
  {
    title: 'Pricing',
    href: '#'
  }
]

const NavbarPage = () => {
  return (
    <div className='h-140'>
      <Navbar navigationData={navigationItems} />
    </div>
  )
}

export default NavbarPage
