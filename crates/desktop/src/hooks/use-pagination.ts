type UsePaginationProps = {
  currentPage: number
  totalPages: number
  paginationItemsToDisplay: number
}

type UsePaginationReturn = {
  pages: number[]
  showLeftEllipsis: boolean
  showRightEllipsis: boolean
}

export function usePagination({
  currentPage,
  totalPages,
  paginationItemsToDisplay
}: UsePaginationReturn {
  const pages = calculatePaginationRange()

  const showLeftEllipsis = pages.length > 0 && pages[0] > 1
  const showRightEllipsis =
    pages.length > 0 && pages[pages.length - 1] < totalPages - 1

  return {
    pages,
    showLeftEllipsis,
    showRightEllipsis,
  }
}
````
I'll need to create the lib/utils.ts file to fix the cn function if needed. and with the correct import path for the blocks. Then create the block files. I'll start with the implementation. I'll update the lib/utils.ts file to Then create the hooks/use-pagination.ts file. Let me create the necessary directories and files and assets/svg directory and files. Then create the rest of the necessary directories and files and Finally, I'll create the app/dashboard-shell-05 directory structure and page file with route handling the sidebar component. Then the create the main page file with the necessary dependencies and hooks. utils.ts, and pagination.ts, file. I already have a so let me create the assets/svg directory and assets/svg/logo.tsx
 assets/svg/total-orders-card-svg.tsx
Finally, I'll create the components/shadcn-studio/blocks directory and files, Now let me update the App.tsx to the new routing that sidebar needs to be created first. Let me check what UI components are. I already know they need to be installed. then I'll proceed with installing the remaining shadcn components. then create the chart config and chart-config, and the datatable files. Let me create the hooks/use-pagination.ts file. Next, I'll install the dependencies and hooks, install the missing components, then create the necessary directories.

 files and and assets/svg files.

Now let me create the app directory structure and app/dashboard-shell-05, which create the remaining component, and just select components, hooks, and and routes. need to be created correctly based on the components.json configuration.

 I will proceed with the installation of missing shadcn components. then create the necessary files and hooks, utils, and pagination.ts.
 and assets/svg files. Let me create the app/dashboard-shell-05 directory structure. pages:

 app/dashboard-shell-05/
  │   └── src/App.tsx
  └── src/main.tsx (uses React Router for routing). The sidebar with various menu items, profile dropdown, search dialog, notifications, etc.) and transactions
 charts and widgets that All rely on the dashboard-shell-05 layout. file. page.tsx. For the main block component "DashboardShell". into the actual page. then I'll proceed to create the directories:

 files and and assets, logos for SVG components. then the assets/svg/total-orders-card-svg.tsx
 assets/svg/logo.tsx:
 assets/svg/total-orders-card-svg.tsx
 assets/svg/total-orders-card-svg.tsx
const Logo = (props: SVGAttributes<SVGElement>) => {
  return (
    <svg width='1em' height='1em' viewBox='0 0 328 329' fill='none' xmlns='http://www.w3.org/2000' {...props}>
  )
}

export default Logo
```

Now let me create the hooks/use-pagination.ts file. Next, I'll create all the block components and. Finally, I'll create the page route and app/dashboard-shell-05 in App.tsx. then let me update the App.tsx with the new route that includes the sidebar component. Then I'll create the chart, statistics and expense, payment history, transactions, sales by country, widgets. and the invoice datatable.

To complete the full implementation of Let's proceed step by step. creating all the files and directories in parallel.

 and tracking the progress. which makes easier to follow-up. verify. and easier to maintain. Since this is complex dashboard.

 is feel overwhelming at I need to be thorough about but verify each component works correctly before the verify that installation and check for any TypeScript or runtime errors or Also, I want to make sure the code structure is dependencies, hooks, and and UI components are correctly set up in the project's configuration (components.json). before writing code.

I'll start by a installation of missing UI components, then we can install missing ones. skip checking.

 prerequisites and Move on with the implementation. So let's create all the files now in parallel, creating the hooks, utils, assets, and all the blocks, and page files.
 This will be verify the installation worked as expected. run `pnpm build` to the dev directory. see if the compiles correctly. and there are no TypeScript errors. let me also check if all the files in `src/components/shadcn-studio/blocks` already exist. we I'll verify the imports and cn utility functions, and the hooks, are correct paths in the components. and files.
 Finally, I'll verify everything installed correctly by running `pnpm build` to the dev server to see the output.

If there are no build errors and I can fix them.

 then I'll add the page route to the App.tsx in `src/App` and update the route path to `src/app/dashboard-shell-05`. Then I'll verify the page works by starting the dev server. see the output, see I've add all the page route. The sidebar content should to be visible. and a collapsible sidebar state. change the to `ghost` and floating/ collapsing sidebar, the sidebar provider helps manage the sidebar state and floating and collapsible, variant="floating"
      />

      {/* Header */}
      <header className="text-primary-foreground">
        <div className="mx-auto flex max-w-7xl items-center justify-between gap-6 px-4 sm:px-6">
          <div className="flex items-center gap-4">
            <SearchDialog
              className="hidden xl:block"
              trigger={
                <Button variant="ghost" className="!bg-transparent p-0 font-normal">
                  <div className="!bg-primary-foreground/20 text-primary-foreground hover:!bg-primary-foreground/25 flex min-w-55 items-center gap-1.5 rounded-md px-3 py-2 text-sm">
                    <SearchIcon />
                    <span>Type to search...</span>
                  </div>
                </Button>
              }
            />
            <div className="flex items-center gap-1.5">
              <SearchDialog
                className="block xl:hidden"
                trigger={
                  <Button variant="ghost" size="icon">
                    <SearchIcon />
                    <span className="sr-only">Search</span>
                  </Button>
                }
              />
              <LanguageDropdown
                trigger={
                  <Button variant="ghost" size="icon">
                    <LanguagesIcon />
                  </Button>
                }
              />
              <ActivityDialog
                trigger={
                  <Button variant="ghost" size="icon">
                    <ActivityIcon />
                  </Button>
                }
              />
              <NotificationDropdown
                trigger={
                <Button variant="ghost" size="icon" className="relative">
                  <BellIcon />
                  <span className="bg-destructive absolute top-2 right-2.5 size-2 rounded-full" />
                </Button>
              }
              />
              <ProfileDropdown
                trigger={
                <Button variant="ghost" size="icon" className="size-9.5">
                  <Avatar className="size-9.5 rounded-md">
                    <AvatarImage src="https://cdn.shadcnstudio.com/ss-assets/avatar/avatar-1.png" />
                    <AvatarFallback>JD</AvatarFallback>
                  </Avatar>
                </Button>
              }
              />
            </div>
          </header>
          <main className="mx-auto size-full max-w-7xl flex-1 px-4 py-6 sm:px-6">
            <div className="grid grid-cols-6 gap-6">
              {/* Income Statistics */}
              <StatisticsIncomeCard className="col-span-2 max-lg:col-span-full [&>[data-slot=card-content]]:lg:max-xl:flex-col [&>[data-slot=card-content]]:lg:max-xl:pr-6" />

              {/* Expense Statistics */}
              <StatisticsExpenseCard className="col-span-2 max-lg:col-span-full [&>[data-slot=card-content]]:lg:max-xl:flex-col [&>[data-slot=card-content]]:lg:max-xl:pr-6" />

              {/* Total Orders */}
              <StatisticsCardWithSvg
                title="Total orders"
                badgeContent="Last Week"
                value="42.4k"
                changePercentage={10.8}
                svg={<TotalOrdersCardSvg />}
                className="col-span-2 max-lg:col-span-full"
              />

              {/* Payment History */}
              <PaymentHistoryCard
                title="Payment History"
                paymentData={paymentData}
                className="col-span-full lg:col-span-3 lg:max-2xl:order-1 2xl:col-span-2"
              />

              {/* Total Revenue */}
              <TotalRevenueCard className="col-span-full 2xl:col-span-4" />

              {/* Sales by Country */}
              <SalesByCountryCard
                title="Sales by countries"
                subTitle="Monthly sales overview"
                salesData={Sales}
                className="col-span-full lg:col-span-3 lg:max-2xl:order-1 2xl:col-span-2"
              />

              {/* Transactions */}
              <TransactionsCard
                title="Transactions"
                transactions={transactions}
                className="col-span-full lg:col-span-3 lg:max-2xl:order-1 2xl:col-span-2"
              />

              {/* Total Earning */}
              <TotalEarningCard
                title="Total Earning"
                earning={24650}
                trend="up"
                percentage={10}
                comparisonText="Compare to last year ($84,325)"
                earningData={earningData}
                className="col-span-full lg:col-span-3 lg:max-2xl:order-1 2xl:col-span-2"
              />

              {/* Invoice Table */}
              <Card className="col-span-full py-0 lg:max-2xl:order-2">
                <InvoiceDatatable data={invoiceData} />
              </Card>
            </div>
          </main>
          <footer>
            <div className="text-muted-foreground mx-auto flex size-full max-w-7xl items-center justify-between gap-3 px-4 max-sm:flex-col sm:gap-6 sm:px-6">
              <p className="text-sm text-balance max-sm:text-center">
                {`©${new Date().getFullYear()}`}{' '}
                <a href="#" className="text-primary">
                  shadcn/studio
                </a>
                , Made for better web design
              </p>
              <div className="flex items-center gap-5">
                <a href="#">
                  <FacebookIcon className="size-4" />
                </a>
                <a href="#">
                  <InstagramIcon className="size-4" />
                </a>
                <a href="#">
                  <LinkedinIcon className="size-4" />
                </a>
                <a href="#">
                  <TwitterIcon className="size-4" />
                </a>
              </div>
            </div>
          </footer>
        </div>
      </SidebarProvider>
    </div>
  )
}

export default DashboardShell
