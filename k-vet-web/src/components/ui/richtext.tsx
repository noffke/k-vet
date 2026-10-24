import { EditorContent, useEditor, useEditorState } from '@tiptap/react'
import { Bold, Italic, List, ListOrdered, type LucideIcon, Underline } from 'lucide-react'
import type { Ref } from 'react'
import { useEffect, useId, useImperativeHandle, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import type { RichText } from '@/api/generated/model'
import { Button } from '@/components/ui/button'
import { Field, invalidClasses } from '@/components/ui/field'
import { documentOf, editorContent, insertBlock, richTextExtensions } from '@/lib/richText'
import { cn } from '@/lib/utils'

/** What a page can do to the field from outside: put a text block in. */
export interface RichTextHandle {
  /**
   * Inserts the block at the caret, or at the end when the field was never focused, and leaves
   * the caret on the line after it. The change reaches `onChange` like anything typed.
   */
  insert: (content: RichText) => void
}

export interface RichTextFieldProps {
  label: string
  /** The stored text the editor opens with. Uncontrolled after that, like `defaultValue`. */
  defaultValue: RichText | null | undefined
  /** Every change, as the API carries it: `null` once nothing but whitespace is left. */
  onChange: (document: RichText | null) => void
  onBlur?: (() => void) | undefined
  error?: string | undefined
  wrapperClassName?: string | undefined
  ref?: Ref<RichTextHandle> | undefined
}

/**
 * The editable element's attributes. It is a `div`, not a form control, so `htmlFor` cannot
 * label it: `aria-labelledby` names the label (and is what `getByLabel` finds).
 */
function editableAttributes(id: string, invalid: boolean): Record<string, string> {
  return {
    id,
    role: 'textbox',
    'aria-multiline': 'true',
    'aria-labelledby': `${id}-label`,
    ...(invalid ? { 'aria-invalid': 'true' } : {}),
    class: 'rich-text min-h-20 max-h-[60vh] overflow-y-auto px-3 py-2 focus-visible:outline-none',
  }
}

/**
 * A text with bold, italic, underline and bullet or numbered lists — the two texts of an
 * animal's record and the text blocks inserted into them. Typography is applied as the vet
 * types (`->` becomes `→`, see `lib/richText.ts`), and the shortcuts are the usual ones
 * (Ctrl+B, Ctrl+I, Ctrl+U; `- ` or `1. ` at the start of a line starts a list).
 *
 * The toolbar is there for the phone, which has no shortcuts. Its buttons keep the editor
 * focused, so the keyboard stays open and the selection stays where it was.
 */
export function RichTextField({
  label,
  defaultValue,
  onChange,
  onBlur,
  error,
  wrapperClassName,
  ref,
}: RichTextFieldProps) {
  const { t } = useTranslation()
  const id = useId()
  // The editor is made once; the callbacks it calls have to be the latest ones.
  const latest = useRef({ onChange, onBlur })
  latest.current = { onChange, onBlur }
  // Until the vet has been in the field, a block goes at its end rather than at a caret they
  // never put anywhere.
  const focused = useRef(false)

  const editor = useEditor({
    extensions: richTextExtensions,
    content: editorContent(defaultValue),
    onUpdate: ({ editor: current }) => latest.current.onChange(documentOf(current)),
    onFocus: () => {
      focused.current = true
    },
    onBlur: () => latest.current.onBlur?.(),
    editorProps: { attributes: editableAttributes(id, Boolean(error)) },
  })

  // A rejected save marks the text invalid after the editor was made.
  useEffect(() => {
    editor.setOptions({ editorProps: { attributes: editableAttributes(id, Boolean(error)) } })
  }, [editor, id, error])

  useImperativeHandle(
    ref,
    () => ({ insert: (content) => insertBlock(editor, content, focused.current) }),
    [editor],
  )

  const active = useEditorState({
    editor,
    selector: ({ editor: current }) => ({
      bold: current.isActive('bold'),
      italic: current.isActive('italic'),
      underline: current.isActive('underline'),
      bulletList: current.isActive('bulletList'),
      orderedList: current.isActive('orderedList'),
    }),
  })

  const tools: {
    key: keyof typeof active
    icon: LucideIcon
    shortcut?: string
    run: () => void
  }[] = [
    {
      key: 'bold',
      icon: Bold,
      shortcut: 'Control+B',
      run: () => editor.chain().focus().toggleBold().run(),
    },
    {
      key: 'italic',
      icon: Italic,
      shortcut: 'Control+I',
      run: () => editor.chain().focus().toggleItalic().run(),
    },
    {
      key: 'underline',
      icon: Underline,
      shortcut: 'Control+U',
      run: () => editor.chain().focus().toggleUnderline().run(),
    },
    {
      key: 'bulletList',
      icon: List,
      run: () => editor.chain().focus().toggleBulletList().run(),
    },
    {
      key: 'orderedList',
      icon: ListOrdered,
      run: () => editor.chain().focus().toggleOrderedList().run(),
    },
  ]

  return (
    <Field label={label} id={id} error={error} className={wrapperClassName}>
      {() => (
        <div
          className={cn(
            'control-box w-full rounded-control border border-line-strong bg-surface text-ink',
            error && invalidClasses,
          )}
        >
          <div
            role="toolbar"
            aria-label={t('richText.toolbar')}
            aria-controls={id}
            className="flex gap-0.5 border-b border-line px-1 py-1"
          >
            {tools.map(({ key, icon: Icon, shortcut, run }) => (
              <Button
                key={key}
                variant="ghost"
                size="icon"
                className="sm:size-8 aria-pressed:bg-sunken aria-pressed:text-ink"
                aria-label={t(`richText.${key}`)}
                title={t(`richText.${key}`)}
                aria-pressed={active[key]}
                aria-keyshortcuts={shortcut}
                // Keeps the focus — and with it the selection and the phone's keyboard — in
                // the editor while the button is pressed.
                onMouseDown={(event) => event.preventDefault()}
                onClick={run}
              >
                <Icon className="size-4" />
              </Button>
            ))}
          </div>
          <EditorContent editor={editor} />
        </div>
      )}
    </Field>
  )
}
