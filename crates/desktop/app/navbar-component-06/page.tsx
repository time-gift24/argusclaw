import Navbar from '@/components/shadcn-studio/blocks/navbar-component-06/navbar-component-06'

const navigationItems = [
  {
    title: 'Home',
    href: '#',
    isActive: true
  }
]

const NavbarPage = () => {
  return (
    <div className='h-32'>
      <Navbar navigationItems={navigationItems} />
    </div>
  )
}

export default NavbarPage
