/** Whether a mouse button, pen or finger is down right now — kept for `closeWhenReleased`. */
let pointerDown = false
if (typeof window !== 'undefined') {
  window.addEventListener(
    'pointerdown',
    () => {
      pointerDown = true
    },
    true,
  )
  window.addEventListener(
    'pointerup',
    () => {
      pointerDown = false
    },
    true,
  )
  window.addEventListener(
    'pointercancel',
    () => {
      pointerDown = false
    },
    true,
  )
}

/**
 * Runs `close` now — or, when a press elsewhere took the focus, once that press is released.
 * Whatever closes on blur and shrinks the page with it (an open value's subline, the picker's
 * results) would otherwise move what was pressed away from under the pointer between press and
 * release, and the click was lost.
 */
export function closeWhenReleased(close: () => void): void {
  if (!pointerDown) {
    close()
    return
  }
  window.addEventListener('pointerup', () => setTimeout(close, 0), { once: true })
}
