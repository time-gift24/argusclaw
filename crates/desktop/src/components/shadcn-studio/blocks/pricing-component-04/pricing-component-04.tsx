import type { ComponentType } from 'react'

import { CheckIcon, CircleIcon } from 'lucide-react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import { Card, CardContent } from '@/components/ui/card'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'

import { cn } from '@/lib/utils'
import { NumberTicker } from '@/components/ui/number-ticker'

type Plans = {
  icon: ComponentType
  title: string
  description: string
  price: number
  period: string
  buttonText: string
  features: string[]
  extraFeatures?: string[]
  isPopular?: boolean
}[]

const Pricing = ({ plans }: { plans: Plans }) => {
  return (
    <div className='relative overflow-hidden py-8 sm:py-16 lg:py-24'>
      <div className='mx-auto max-w-7xl px-4 sm:px-6 lg:px-8'>
        <div className='mb-12 space-y-4 text-center sm:mb-16 lg:mb-24'>
          <h2 className='text-2xl font-bold md:text-3xl lg:text-4xl'>
            <span className='relative z-10'>
              Pricing Details
              <span
                className='from-primary absolute bottom-0 left-0 -z-10 h-0.5 w-full bg-gradient-to-r to-transparent'
                aria-hidden='true'
              />
            </span>
          </h2>
          <p className='text-muted-foreground text-xl'>
            A Comprehensive Breakdown of Our Pricing Plans to Help You Make the Best Choice!
          </p>
        </div>

        <div className='flex justify-center gap-6 max-sm:flex-col max-sm:items-center [&>*]:h-fit'>
          {plans.map((plan, index) => (
            <Card
              key={index}
              className={cn('w-full max-w-95 rounded-3xl p-2 pb-4 shadow-none', { 'bg-muted': plan.isPopular })}
            >
              <CardContent
                className={cn('bg-muted flex flex-col gap-6 rounded-3xl p-6', {
                  'bg-background relative overflow-hidden shadow-lg': plan.isPopular
                })}
              >
                <div className={cn({ 'flex items-start justify-between': plan.isPopular })}>
                  <Avatar className='bg-primary/10 size-12 rounded-md border shadow-md'>
                    <AvatarFallback
                      className={`${plan.isPopular ? 'bg-muted text-foreground' : 'bg-white text-black'} rounded-md shadow-md`}
                    >
                      <plan.icon />
                    </AvatarFallback>
                  </Avatar>
                  {plan.isPopular && (
                    <Badge variant='destructive' className='z-10'>
                      Trending
                    </Badge>
                  )}
                </div>

                <div className='flex-1 space-y-2.5'>
                  <h3 className='text-2xl font-semibold'>{plan.title}</h3>
                  <p>{plan.description}</p>
                </div>

                <p className='text-primary text-5xl font-bold'>
                  $<NumberTicker startValue={0} value={plan.price} />
                  <span className='text-muted-foreground ml-0.75 text-lg font-normal'>{plan.period}</span>
                </p>

                <Button size='lg' variant={plan.isPopular ? 'default' : 'outline'} className='w-full rounded-full'>
                  {plan.buttonText}
                </Button>
              </CardContent>
              <div className='space-y-4'>
                <ul className='space-y-1.5 px-4'>
                  {plan.features.map((feature, featureIndex) => (
                    <li key={featureIndex} className='flex items-center gap-2 py-1'>
                      <CheckIcon className='text-primary size-3.5 shrink-0' />
                      <span>{feature}</span>
                    </li>
                  ))}
                </ul>

                {plan.extraFeatures && plan.extraFeatures.length > 0 && (
                  <>
                    <Separator className='-mx-2 !w-auto' />
                    <ul className='space-y-1.5 px-4'>
                      {plan.extraFeatures.map((feature, featureIndex) => (
                        <li key={featureIndex} className='flex items-center gap-2 py-1'>
                          <CircleIcon className='fill-primary text-primary size-2.5 shrink-0' />
                          <span>{feature}</span>
                        </li>
                      ))}
                    </ul>
                  </>
                )}
              </div>
            </Card>
          ))}
        </div>
      </div>
    </div>
  )
}

export default Pricing
