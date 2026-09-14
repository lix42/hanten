import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import type { JSX } from "solid-js";
import { css } from "../styled-system/css";
import { formatNotes, noteCount, type NotedFrame, type Notes } from "./notes";

/**
 * The note dialogs: one frame's note (`a`), every note (`n`), and the guard on
 * clearing them (`c`).
 *
 * Built on the native `<dialog>` so Esc, the backdrop and focus containment come
 * from the platform rather than from hand-rolled key handling — `esc` closing the
 * window is then simply what a modal dialog does.
 *
 * The textareas are **uncontrolled**: their initial value is written once by the
 * ref and the note signal is updated on input, never written back. Binding
 * `value` to the signal instead re-assigns the property on every keystroke, which
 * moves the caret to the end mid-word. Contents are mounted only while the
 * dialog is open, so each opening starts from the current note.
 */

const styles = {
  dialog: css.raw({
    padding: "0",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "edge",
    borderRadius: "lg",
    backgroundColor: "panel",
    color: "fg",
    maxHeight: "dialogTall",
    overflow: "hidden",
    "&::backdrop": { backgroundColor: "scrim" },
  }),
  narrow: css.raw({ width: "dialogNarrow" }),
  wide: css.raw({ width: "dialogWide" }),
  frame: css.raw({
    display: "flex",
    flexDirection: "column",
    maxHeight: "dialogTall",
    minHeight: "zero",
  }),
  head: css.raw({
    display: "flex",
    gap: "12px",
    alignItems: "baseline",
    flexWrap: "wrap",
    paddingBlock: "12px",
    paddingInline: "16px",
    borderBottomWidth: "1px",
    borderBottomStyle: "solid",
    borderBottomColor: "edge",
    flexShrink: 0,
  }),
  title: css.raw({ fontWeight: "semibold" }),
  sub: css.raw({ color: "fg.dim", fontSize: "meta" }),
  body: css.raw({
    padding: "16px",
    overflowY: "auto",
    minHeight: "zero",
    display: "flex",
    flexDirection: "column",
    gap: "12px",
  }),
  foot: css.raw({
    display: "flex",
    gap: "8px",
    alignItems: "center",
    paddingBlock: "12px",
    paddingInline: "16px",
    borderTopWidth: "1px",
    borderTopStyle: "solid",
    borderTopColor: "edge",
    flexShrink: 0,
  }),
  spacer: css.raw({ marginInlineStart: "auto" }),
  textarea: css.raw({
    width: "full",
    boxSizing: "border-box",
    resize: "vertical",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "edge",
    borderRadius: "md",
    backgroundColor: "bg",
    color: "fg",
    fontFamily: "body",
    fontSize: "body",
    lineHeight: "body",
    padding: "8px",
  }),
  tall: css.raw({ minHeight: "noteBox" }),
  short: css.raw({ minHeight: "noteRow" }),
  row: css.raw({ display: "flex", flexDirection: "column", gap: "4px" }),
  rowLabel: css.raw({ fontSize: "meta", fontWeight: "semibold" }),
  noted: css.raw({ color: "accent" }),
  button: css.raw({
    paddingBlock: "5px",
    paddingInline: "12px",
    borderRadius: "md",
    borderWidth: "1px",
    borderStyle: "solid",
    borderColor: "edge",
    backgroundColor: "button",
    color: "fg",
    fontFamily: "body",
    fontSize: "body",
    cursor: "pointer",
  }),
  primary: css.raw({ backgroundColor: "accent", borderColor: "accent", color: "accent.ink" }),
  danger: css.raw({ color: "bad" }),
  status: css.raw({ fontSize: "meta", color: "fg.dim" }),
  failed: css.raw({ fontSize: "meta", color: "bad" }),
  empty: css.raw({ color: "fg.dim" }),
};

/** A modal whose contents exist only while it is open. */
function Modal(props: {
  open: boolean;
  onClose: () => void;
  wide?: boolean;
  label: string;
  children: JSX.Element;
}) {
  let element: HTMLDialogElement | undefined;
  createEffect(() => {
    if (!element) return;
    if (props.open && !element.open) element.showModal();
    else if (!props.open && element.open) element.close();
  });
  // A dialog left open when the page navigates away keeps the document inert.
  onCleanup(() => element?.open && element.close());
  return (
    <dialog
      ref={(node) => (element = node)}
      aria-label={props.label}
      class={css(styles.dialog, props.wide ? styles.wide : styles.narrow)}
      onClose={() => props.onClose()}
    >
      <Show when={props.open}>
        <div class={css(styles.frame)}>{props.children}</div>
      </Show>
    </dialog>
  );
}

function Textarea(props: {
  value: string;
  onInput: (text: string) => void;
  short?: boolean;
  autofocus?: boolean;
  label: string;
}) {
  return (
    <textarea
      class={css(styles.textarea, props.short ? styles.short : styles.tall)}
      aria-label={props.label}
      ref={(element) => {
        element.value = props.value;
        if (props.autofocus) queueMicrotask(() => element.focus());
      }}
      onInput={(event) => props.onInput(event.currentTarget.value)}
    />
  );
}

/** `a` — the current frame's note. */
export function NoteDialog(props: {
  open: boolean;
  onClose: () => void;
  frame: NotedFrame | undefined;
  note: string;
  onInput: (text: string) => void;
}) {
  return (
    <Modal open={props.open} onClose={props.onClose} label="Note for this frame">
      <div class={css(styles.head)}>
        <span class={css(styles.title)}>Note</span>
        <span class={css(styles.sub)}>{props.frame?.label ?? "no frame"}</span>
      </div>
      <div class={css(styles.body)}>
        <Textarea
          value={props.note}
          onInput={props.onInput}
          autofocus
          label={`Note for ${props.frame?.label ?? "this frame"}`}
        />
      </div>
      <div class={css(styles.foot)}>
        <span class={css(styles.status)}>Saved as you type. Esc closes.</span>
        <button type="button" class={css(styles.button, styles.spacer)} onClick={props.onClose}>
          Close
        </button>
      </div>
    </Modal>
  );
}

/** `n` — every note, editable, with the whole set as one copyable block. */
export function NotesDialog(props: {
  open: boolean;
  onClose: () => void;
  frames: readonly NotedFrame[];
  notes: Notes;
  title: string | undefined;
  onInput: (id: string, text: string) => void;
}) {
  const [copied, setCopied] = createSignal(false);
  const [failed, setFailed] = createSignal<string | undefined>(undefined);
  const text = () => formatNotes(props.frames, props.notes, props.title);

  // Held here, not registered inside `copy`: that runs from a DOM handler and
  // then awaits, so by the time it resumed there was no reactive owner to
  // register a cleanup on and the call did nothing but look like a guard.
  let clearCopied: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(clearCopied));

  const copy = async () => {
    setFailed(undefined);
    try {
      await navigator.clipboard.writeText(text());
      setCopied(true);
      clearTimeout(clearCopied);
      clearCopied = setTimeout(() => setCopied(false), 1500);
    } catch (cause) {
      // Said out loud rather than swallowed: a silent no-op here looks exactly
      // like a successful copy, and the notes are the point of the feature.
      setFailed(cause instanceof Error ? cause.message : String(cause));
    }
  };

  return (
    <Modal open={props.open} onClose={props.onClose} wide label="All review notes">
      <div class={css(styles.head)}>
        <span class={css(styles.title)}>Notes</span>
        <span class={css(styles.sub)}>
          {noteCount(props.notes)} of {props.frames.length} frames noted
        </span>
      </div>
      <div class={css(styles.body)}>
        <Show
          when={props.frames.length > 0}
          fallback={<span class={css(styles.empty)}>This review set lists no frames.</span>}
        >
          <For each={props.frames}>
            {(frame) => (
              <div class={css(styles.row)}>
                <span
                  class={css(
                    styles.rowLabel,
                    (props.notes[frame.id] ?? "").trim() !== "" && styles.noted,
                  )}
                >
                  {frame.label}
                </span>
                <Textarea
                  value={props.notes[frame.id] ?? ""}
                  onInput={(value) => props.onInput(frame.id, value)}
                  short
                  label={`Note for ${frame.label}`}
                />
              </div>
            )}
          </For>
        </Show>
      </div>
      <div class={css(styles.foot)}>
        <button
          type="button"
          class={css(styles.button, styles.primary)}
          disabled={text() === ""}
          onClick={() => void copy()}
        >
          Copy all
        </button>
        <Show when={copied()}>
          <span class={css(styles.status)}>Copied.</span>
        </Show>
        <Show when={failed()}>
          {(why) => <span class={css(styles.failed)}>Copy failed — {why()}</span>}
        </Show>
        <Show when={text() === ""}>
          <span class={css(styles.status)}>Nothing noted yet.</span>
        </Show>
        <button type="button" class={css(styles.button, styles.spacer)} onClick={props.onClose}>
          Close
        </button>
      </div>
    </Modal>
  );
}

/** `c` — clearing every note is unrecoverable, so it is asked for once. */
export function ClearDialog(props: {
  open: boolean;
  onClose: () => void;
  count: number;
  onConfirm: () => void;
}) {
  return (
    <Modal open={props.open} onClose={props.onClose} label="Clear all notes">
      <div class={css(styles.head)}>
        <span class={css(styles.title)}>Clear all notes?</span>
      </div>
      <div class={css(styles.body)}>
        <span>
          {props.count} {props.count === 1 ? "note" : "notes"} will be discarded. Notes live only in
          this page, so there is nothing to restore them from.
        </span>
      </div>
      <div class={css(styles.foot)}>
        <button
          type="button"
          class={css(styles.button, styles.danger)}
          onClick={() => {
            props.onConfirm();
            props.onClose();
          }}
        >
          Clear {props.count}
        </button>
        <button type="button" class={css(styles.button, styles.spacer)} onClick={props.onClose}>
          Cancel
        </button>
      </div>
    </Modal>
  );
}
