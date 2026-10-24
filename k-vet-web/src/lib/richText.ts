import type { AnyExtension, Editor, JSONContent } from '@tiptap/core'
import { ListItem } from '@tiptap/extension-list'
import Typography from '@tiptap/extension-typography'
import type { Node as ProseMirrorNode } from '@tiptap/pm/model'
import { TextSelection } from '@tiptap/pm/state'
import StarterKit from '@tiptap/starter-kit'
import type { RichText } from '@/api/generated/model'

/**
 * Typography as the vet types, set for German — the document is always German (FR-030), so
 * quotes are „…“ whatever language the app is shown in. Each rule can be undone with Backspace
 * straight after it fired.
 *
 * Off: ©, ®, ™, ℠ and «» — `(c)` is the third point of an enumeration in a clinical note far more
 * often than a copyright sign. Opening single quotes are left alone, so `'` always becomes the
 * apostrophe ’ ("geht’s"), which German writes far more often than a single quotation mark.
 */
const typographyRules = {
  rightArrow: '→',
  leftArrow: '←',
  // The German dash is the en dash, set with spaces: "Ohr – links".
  emDash: '–',
  ellipsis: '…',
  openDoubleQuote: '„',
  closeDoubleQuote: '“',
  openSingleQuote: false,
  closeSingleQuote: '’',
  oneHalf: '½',
  oneQuarter: '¼',
  threeQuarters: '¾',
  multiplication: '×',
  plusMinus: '±',
  notEqual: '≠',
  superscriptTwo: '²',
  superscriptThree: '³',
  copyright: false,
  registeredTrademark: false,
  trademark: false,
  servicemark: false,
  laquo: false,
  raquo: false,
} as const

/**
 * What the editor can write — exactly what the server stores (`domain::rich_text`) and the
 * invoice prints: paragraphs with bold, italic and underline and line breaks, and bullet or
 * numbered lists one level deep. Pasted text is cut down to this as well, by the schema itself.
 */
export const richTextExtensions: AnyExtension[] = [
  StarterKit.configure({
    blockquote: false,
    code: false,
    codeBlock: false,
    dropcursor: false,
    heading: false,
    horizontalRule: false,
    link: false,
    listItem: false,
    strike: false,
  }),
  // An item holds paragraphs, never another list: Tab does not indent, and nothing deeper than
  // the server accepts can be written.
  ListItem.extend({ content: 'paragraph+' }),
  Typography.configure(typographyRules),
]

/**
 * The editor's document as the API carries it, or `null` when nothing but whitespace is in
 * it — an emptied field is no text, not a document of empty paragraphs (the server stores
 * NULL either way, and the customer lock counts what is not null).
 *
 * The cast is safe because the schema above is the server's: whatever the editor holds is a
 * {@link RichText}.
 */
export function documentOf(editor: Editor): RichText | null {
  if (editor.getText().trim() === '') return null
  return editor.getJSON() as unknown as RichText
}

/** The stored document as the editor's initial content. */
export function editorContent(document: RichText | null | undefined): JSONContent | null {
  return (document ?? null) as unknown as JSONContent | null
}

/**
 * Puts a text block's content in at the caret — at the end when the field was never focused —
 * as paragraphs of their own, and leaves the caret on the line after them, ready for the next
 * block or for typing (issues.md 3, the behaviour the plain textarea had):
 *
 * - in the middle of a paragraph, the paragraph is split and the block goes between the
 *   halves; the caret starts the second half;
 * - at the start of a paragraph, the block goes before it;
 * - at the end of one, or after a list, the block goes after it, followed by an empty line;
 * - in an empty paragraph, the block goes before it and the caret stays in the empty line.
 */
export function insertBlock(editor: Editor, content: RichText, atCaret: boolean) {
  const blocks = (editorContent(content)?.content ?? []).map((json) =>
    editor.schema.nodeFromJSON(json),
  )
  if (blocks.length === 0) return
  const size = blocks.reduce((total, node) => total + node.nodeSize, 0)

  editor
    .chain()
    .focus(atCaret ? undefined : 'end')
    .command(({ tr }) => {
      const { $from } = tr.selection
      // The top-level block the caret is in: a paragraph, or a whole list.
      const top: ProseMirrorNode | null = $from.depth > 0 ? $from.node(1) : null
      const start = $from.depth > 0 ? $from.before(1) : tr.doc.content.size
      const end = $from.depth > 0 ? $from.after(1) : tr.doc.content.size
      const inParagraph = top?.type.name === 'paragraph'
      const textBefore = inParagraph && $from.parentOffset > 0
      const textAfter = inParagraph && $from.parentOffset < $from.parent.content.size

      let caret: number
      if (textBefore && textAfter) {
        tr.split($from.pos)
        // The first half now ends at `$from.pos`; the second opens right after it.
        tr.insert($from.pos + 1, blocks)
        caret = $from.pos + 1 + size + 1
      } else if (inParagraph && !textBefore) {
        tr.insert(start, blocks)
        caret = start + size + 1
      } else {
        const line = editor.schema.nodes.paragraph?.create()
        tr.insert(end, line ? [...blocks, line] : blocks)
        caret = end + size + 1
      }
      tr.setSelection(TextSelection.near(tr.doc.resolve(caret))).scrollIntoView()
      return true
    })
    .run()
}
