/**
 * Passed as `ref` to a picker's `Command.Input`: keeps that picker from pulling the focus out
 * of another one.
 *
 * cmdk (1.1.1) focuses its own input whenever its selection changes while the focused element
 * carries `cmdk-input` — *any* picker's input, not only its own. Two pickers on one page (the
 * Preisauskunft's customer and position search, an appointment's animal and position search)
 * then fight: a list that finishes loading late pulls the vet's typing into its own field. cmdk
 * reads the attribute for that check alone and finds its input by id, so dropping it keeps each
 * picker to itself. `tests/command.test.tsx` says when cmdk no longer needs this.
 */
export function unmarkCmdkInput(node: HTMLInputElement | null): void {
  node?.removeAttribute('cmdk-input')
}
