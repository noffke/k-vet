# Various Issues

- on the customers page, search box, please also allow to search for street, phone number and patient name
- on the customers page, customers table, please restructure: drop the city, phone, email columns. instead, add a comma separated list of patient names, alphabetically sorted, without deceased patients. in a second, smaller line in the row (unrestricted by the inner columns) show the address 
- on the pharmacy page, drug table. drop the vat column, instead add a column for the gross price of the original packaging, so the vet can quickly look up a price to tell to a customer
- on the drug detail page, please highlight the gross sales price so it is more easily visible (for giving the number to a customer asking)
- on a treatment, when adding positions, there is no clear visual feedback anything happened because most positions get added outside of the viewport. maybe a toast? any other such places where such visual feedback would be helpful?
- on the customer page table, the first column header is "last name", but it is actually showing the full name, so maybe just "Name"?
- please remove the patients page completely. this also means that when editing a patient, the "back" should go back to the customer (this was inconsistent before, that it would jump to the patients page)
- on the invoice, please use a sans serif font
- we need a new entity: a text block (name, content) with a library so that it is faster to enter findings and treatment reasons. no need for a relation of a text block to where it was used. we then need a new main page that allows CRUD for the entity. on the patient treatment page, both for reason of treatment and finding we need the possibility to use the text block library. if possible, select a text block and insert at cursor position, otherwise insert at end. the selection "dialog" should have the option to search for text block name or content (like the other search boxes)
- Wording change (in web and invoice): "Behandlungsgrund" -> "Vorbericht und Untersuchung", "Befund" -> "Therapie und weiteres Vorgehen"
- please add the create invoice box also to the patient treatment page. this is the most common case, one patient treatment per appointment. currently, you have to jump back to the appointment to create the invoice
- on the create invoice "dialog": please change the wording from "Notiz" zu "Notiz (intern)"
- on the accept invoice "dialog": you currently have to type in the email address, but we already store the customer's email address. so please make this a multi select prefilled with all known email addresses of the customer but allow to enter an additional email address not stored. when adding a new address here, ask if the address should be added to the customer data and do so if confirmed.
- on the invoice: the Einzelpreis must already be with the factor applied
- on the invoice: please move the payment section to the end of the document
- on the invoices page: one column header reads "FIELD.TOTAL". please make sure that no translation placeholders are visible in the frontend. if possible, add a test that ensures this.
- on a patient's page, please link all the patient treatments they were part of at the bottom
- for an invoice, we need at least first name, last name, street, zip, city, country. in the customers list, please mark customers where one of these fields are not filled. on the edit customer page, please also highlight the missing fields. when creating an invoice, please show a confirmation dialog if one of the required fields is missing.