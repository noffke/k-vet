import { Editor, type JSONContent } from '@tiptap/core'
import { afterEach, describe, expect, it } from 'vitest'
import type { RichText } from '@/api/generated/model'
import { documentOf, insertBlock, richTextExtensions } from '@/lib/richText'

let editor: Editor | undefined

afterEach(() => {
  editor?.destroy()
  editor = undefined
})

function open(content: JSONContent | null) {
  editor = new Editor({ extensions: richTextExtensions, content })
  return editor
}

const paragraph = (text?: string): JSONContent =>
  text ? { type: 'paragraph', content: [{ type: 'text', text }] } : { type: 'paragraph' }

const doc = (...content: JSONContent[]): JSONContent => ({ type: 'doc', content })

/** The paragraphs' texts, list items flattened — enough to see where a block landed. */
function lines(current: Editor) {
  const texts: string[] = []
  current.state.doc.descendants((node) => {
    if (node.type.name === 'paragraph') texts.push(node.textContent)
  })
  return texts
}

/** The text before the caret in its paragraph, and after it. */
function caret(current: Editor) {
  const { $from } = current.state.selection
  const text = $from.parent.textContent
  return [text.slice(0, $from.parentOffset), text.slice($from.parentOffset)]
}

/** Types the way a keyboard does, so the input rules see each character arrive. */
function type(current: Editor, text: string) {
  for (const character of text) {
    const { view, state } = current
    const { from, to } = state.selection
    const handled = view.someProp('handleTextInput', (handle) =>
      handle(view, from, to, character, () => state.tr.insertText(character, from, to)),
    )
    if (!handled) view.dispatch(view.state.tr.insertText(character, from, to))
  }
}

const block: RichText = {
  type: 'doc',
  content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Baustein' }] }],
} as RichText

describe('documentOf', () => {
  it('is null for an editor holding nothing but whitespace', () => {
    expect(documentOf(open(null))).toBeNull()
    expect(documentOf(open(doc(paragraph(), paragraph('   '))))).toBeNull()
  })

  it('is the document once there is text', () => {
    const document = documentOf(open(doc(paragraph('Ohr'))))
    expect(document?.content).toEqual([paragraph('Ohr')])
  })
})

describe('insertBlock', () => {
  it('splits a paragraph and leaves the caret at the start of its second half', () => {
    const current = open(doc(paragraph('AnfangEnde')))
    current.commands.setTextSelection(7)
    insertBlock(current, block, true)
    expect(lines(current)).toEqual(['Anfang', 'Baustein', 'Ende'])
    expect(caret(current)).toEqual(['', 'Ende'])
  })

  it('goes after a paragraph the caret ends, onto a fresh line', () => {
    const current = open(doc(paragraph('Anfang'), paragraph('Weiter')))
    current.commands.setTextSelection(7)
    insertBlock(current, block, true)
    expect(lines(current)).toEqual(['Anfang', 'Baustein', '', 'Weiter'])
    expect(caret(current)).toEqual(['', ''])
  })

  it('goes before a paragraph the caret starts', () => {
    const current = open(doc(paragraph('Ende')))
    current.commands.setTextSelection(1)
    insertBlock(current, block, true)
    expect(lines(current)).toEqual(['Baustein', 'Ende'])
    expect(caret(current)).toEqual(['', 'Ende'])
  })

  it('fills an empty field and keeps a fresh line under the block', () => {
    const current = open(null)
    insertBlock(current, block, false)
    expect(lines(current)).toEqual(['Baustein', ''])
    expect(caret(current)).toEqual(['', ''])
  })

  it('goes to the end of a field that was never focused', () => {
    const current = open(doc(paragraph('Anfang'), paragraph('Ende')))
    insertBlock(current, block, false)
    expect(lines(current)).toEqual(['Anfang', 'Ende', 'Baustein', ''])
  })

  it('keeps a formatted block as it is', () => {
    const list = {
      type: 'doc',
      content: [
        {
          type: 'orderedList',
          attrs: { start: 1 },
          content: [{ type: 'listItem', content: [paragraph('spülen')] }],
        },
      ],
    } as RichText
    const current = open(null)
    insertBlock(current, list, false)
    expect(current.getJSON().content?.[0]?.type).toBe('orderedList')
  })
})

describe('typography', () => {
  it.each([
    ['->', '→'],
    ['<-', '←'],
    ['Ohr -- links', 'Ohr – links'],
    ['warten...', 'warten…'],
    ['"gut"', '„gut“'],
    ["geht's", 'geht’s'],
    ['3x2', '3×2'],
    ['+/-', '±'],
    ['cm^2', 'cm²'],
    ['1/2 ', '½ '],
  ])('turns %j into %j', (typed, shown) => {
    const current = open(null)
    type(current, typed)
    expect(current.getText()).toBe(shown)
  })

  it('leaves the signs a clinical note does not want alone', () => {
    const current = open(null)
    type(current, '(a) (b) (c) <<x>>')
    expect(current.getText()).toBe('(a) (b) (c) <<x>>')
  })
})
