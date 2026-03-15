import Navbar from '@/components/shadcn-studio/blocks/navbar-component-06/navbar-component-06'

const navigationData = [
  {
    title: 'Home',
    href: '#'
  },
  {
    title: 'Marketing',
    href: '#'
  },
  {
    title: 'Contact',
    href: '#'
  },
  {
    title: 'Projects',
    href: '#'
  },
  {
    title: 'Sales',
    href: '#'
  },
  {
    title: 'Team',
    href: '#'
  },
  {
    title: 'Tasks',
    href: '#'
  },
  {
    title: 'Blog',
    href: '#'
  }
]

const navigationItems = [
  {
    title: 'Home',
    href: '#'
  },
  {
    title: 'Marketing',
    href: '#',
    isActive: true
  },
  {
    title: 'Contact',
    href: '#'
  }
]

const NavbarPage = () => {
  return (
    <div className='h-132'>
      <Navbar navigationData={navigationData} navigationItems={navigationItems} />
    </div>
  )
}

export default NavbarPage
