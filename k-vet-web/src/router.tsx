import {
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  redirect,
} from '@tanstack/react-router'
import { AppLayout } from '@/components/AppLayout'
import { AppointmentDetailPage } from '@/features/appointments/AppointmentDetailPage'
import { AppointmentsPage } from '@/features/appointments/AppointmentsPage'
import { LoginPage } from '@/features/auth/LoginPage'
import { sessionQueryOptions } from '@/features/auth/session'
import { CustomerDetailPage } from '@/features/customers/CustomerDetailPage'
import { CustomersPage } from '@/features/customers/CustomersPage'
import { DashboardPage } from '@/features/dashboard/DashboardPage'
import { InvoicesPage } from '@/features/invoices/InvoicesPage'
import {
  ManufacturerDetailPage,
  SupplierDetailPage,
} from '@/features/masterdata/AddressBookDetailPage'
import { MasterDataPage } from '@/features/masterdata/MasterDataPage'
import { PatientDetailPage } from '@/features/patients/PatientDetailPage'
import { PatientsPage } from '@/features/patients/PatientsPage'
import { DrugDetailPage } from '@/features/pharmacy/DrugDetailPage'
import { LotDetailPage } from '@/features/pharmacy/LotDetailPage'
import { PharmacyPage } from '@/features/pharmacy/PharmacyPage'
import { StockCorrectionPage } from '@/features/pharmacy/StockCorrectionPage'
import { StockIntakePage } from '@/features/pharmacy/StockIntakePage'
import { ServiceDetailPage, ServicesPage } from '@/features/services/ServicesPage'
import { SettingsPage } from '@/features/settings/SettingsPage'
import { NotFoundPage } from '@/features/shell/NotFoundPage'
import { TemplateDetailPage, TemplatesPage } from '@/features/templates/TemplatesPage'
import { InvoiceAcceptPage } from '@/features/treatments/InvoiceAcceptPage'
import { InvoiceCreatePage } from '@/features/treatments/InvoiceCreatePage'
import { InvoiceSendPage } from '@/features/treatments/InvoiceSendPage'
import { PatientTreatmentPage } from '@/features/treatments/PatientTreatmentPage'
import { TreatmentPage } from '@/features/treatments/TreatmentPage'
import { queryClient } from '@/lib/query'

const rootRoute = createRootRoute({
  component: () => <Outlet />,
  notFoundComponent: NotFoundPage,
})

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/login',
  component: LoginPage,
})

/**
 * Everything below the layout route requires a session. The guard resolves the session
 * query once and reuses the cache, so a reload does not flash the login screen.
 */
const appRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: 'app',
  component: AppLayout,
  beforeLoad: async () => {
    const session = await queryClient.ensureQueryData(sessionQueryOptions())
    if (!session.authenticated) throw redirect({ to: '/login' })
  },
})

const invoicesRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/invoices',
  component: InvoicesPage,
})

const settingsRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/settings',
  component: SettingsPage,
})

const dashboardRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/',
  component: DashboardPage,
})

const appointmentsRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/appointments',
  component: AppointmentsPage,
})

const appointmentDetailRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/appointments/$id',
  component: AppointmentDetailPage,
})

const customersRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/customers',
  component: CustomersPage,
})

const customerDetailRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/customers/$id',
  component: CustomerDetailPage,
})

const patientsRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/patients',
  component: PatientsPage,
})

const patientDetailRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/patients/$id',
  component: PatientDetailPage,
})

const pharmacyRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/pharmacy',
  component: PharmacyPage,
})

const drugDetailRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/pharmacy/$id',
  component: DrugDetailPage,
})

const stockIntakeRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/pharmacy/$id/intake',
  component: StockIntakePage,
})

const lotDetailRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/lots/$id',
  component: LotDetailPage,
})

const masterDataRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/masterdata',
  component: MasterDataPage,
})

const manufacturerDetailRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/masterdata/manufacturers/$id',
  component: ManufacturerDetailPage,
})

const supplierDetailRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/masterdata/suppliers/$id',
  component: SupplierDetailPage,
})

const stockCorrectionRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/lots/$id/correction',
  component: StockCorrectionPage,
})

const servicesRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/services',
  component: ServicesPage,
})

const serviceDetailRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/services/$id',
  component: ServiceDetailPage,
})

const templatesRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/templates',
  component: TemplatesPage,
})

const templateDetailRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/templates/$id',
  component: TemplateDetailPage,
})

const treatmentRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/treatments/$id',
  component: TreatmentPage,
})

const invoiceCreateRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/treatments/$id/invoice',
  component: InvoiceCreatePage,
})

const invoiceAcceptRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/treatments/$id/invoice/accept',
  component: InvoiceAcceptPage,
})

const invoiceSendRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/treatments/$id/invoice/send',
  component: InvoiceSendPage,
})

const patientTreatmentRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/patient-treatments/$id',
  component: PatientTreatmentPage,
})

const routeTree = rootRoute.addChildren([
  loginRoute,
  appRoute.addChildren([
    dashboardRoute,
    invoicesRoute,
    settingsRoute,
    appointmentsRoute,
    appointmentDetailRoute,
    customersRoute,
    customerDetailRoute,
    patientsRoute,
    patientDetailRoute,
    pharmacyRoute,
    drugDetailRoute,
    stockIntakeRoute,
    lotDetailRoute,
    stockCorrectionRoute,
    masterDataRoute,
    manufacturerDetailRoute,
    supplierDetailRoute,
    servicesRoute,
    serviceDetailRoute,
    templatesRoute,
    templateDetailRoute,
    treatmentRoute,
    invoiceCreateRoute,
    invoiceAcceptRoute,
    invoiceSendRoute,
    patientTreatmentRoute,
  ]),
])

export const router = createRouter({
  routeTree,
  defaultPreload: 'intent',
  scrollRestoration: true,
})

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router
  }
}
