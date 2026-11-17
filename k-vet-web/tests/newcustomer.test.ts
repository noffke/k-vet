import { describe, expect, it } from 'vitest'
import { splitName } from '@/features/customers/NewCustomerDialog'

// What was typed into the appointment's customer search becomes the new customer's name.
describe('splitName', () => {
  it('takes the last word as the last name and the rest as the first name', () => {
    expect(splitName('Erika Muster')).toEqual({ firstName: 'Erika', lastName: 'Muster' })
    expect(splitName('  Anna  Lena   Muster ')).toEqual({
      firstName: 'Anna Lena',
      lastName: 'Muster',
    })
  })

  it('takes a single word as the last name, since that is what is searched for', () => {
    expect(splitName('Muster')).toEqual({ firstName: '', lastName: 'Muster' })
  })

  it('leaves both empty for an empty search', () => {
    expect(splitName('   ')).toEqual({ firstName: '', lastName: '' })
  })
})
