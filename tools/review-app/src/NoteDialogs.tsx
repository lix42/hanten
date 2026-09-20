import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import type { JSX } from "solid-js";
import { css } from "../styled-system/css";
import { formatReview, noteCount, type NotedFrame, type Notes } from "./notes";
import { formatPatch, patchCount, patchesFor, type Patches } from "./patches";

/**
 * The review dialogs: one frame's note (`a`), every note and patch (`n`), the
 * guard on clearing them (`c`), and the label a new patch is given (`p`).
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
  // The patch label is a few words, so it gets a line rather than a box — the
  // shape of the field is what says how much to write.
  input: css.raw({
    width: "full",
    boxSizing: "border-box",
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
  patchList: css.raw({
    margin: "0",
    paddingInline: "16px",
    paddingBlock: "0",
    color: "fg.dim",
    fontFamily: "mono",
    fontSize: "key",
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

/**
 * `n` — every note and patch, with the whole review as one copyable block.
 *
 * The notes are editable here; the **patches are not**. A patch is a rectangle,
 * and the only honest place to change one is on the picture — so they are listed
 * for reading and for copying, and `p` is where they are drawn and deleted.
 */
export function NotesDialog(props: {
  open: boolean;
  onClose: () => void;
  frames: readonly NotedFrame[];
  notes: Notes;
  patches: Patches;
  title: string | undefined;
  onInput: (id: string, text: string) => void;
}) {
  const [copied, setCopied] = createSignal(false);
  const [failed, setFailed] = createSignal<string | undefined>(undefined);
  const text = () => formatReview(props.frames, props.notes, props.patches, props.title);

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
          <Show when={patchCount(props.patches) > 0}>
            {" · "}
            {patchCount(props.patches)} {patchCount(props.patches) === 1 ? "patch" : "patches"}
          </Show>
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
                <Show when={patchesFor(props.patches, frame.id).length > 0}>
                  <ul class={css(styles.patchList)}>
                    <For each={patchesFor(props.patches, frame.id)}>
                      {(patch) => <li>{formatPatch(patch).replace(/^- /, "")}</li>}
                    </For>
                  </ul>
                </Show>
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

/** `c` — clearing what you wrote is unrecoverable, so it is asked for once. */
export function ClearDialog(props: {
  open: boolean;
  onClose: () => void;
  notes: number;
  patches: number;
  onConfirm: () => void;
}) {
  // Both are named, because a review with only patches on it would otherwise be
  // asked to confirm discarding "0 notes" and then lose every rectangle.
  const parts = () =>
    [
      props.notes > 0 ? `${props.notes} ${props.notes === 1 ? "note" : "notes"}` : undefined,
      props.patches > 0
        ? `${props.patches} ${props.patches === 1 ? "patch" : "patches"}`
        : undefined,
    ].filter((part) => part !== undefined);
  return (
    <Modal open={props.open} onClose={props.onClose} label="Clear all notes and patches">
      <div class={css(styles.head)}>
        <span class={css(styles.title)}>Clear everything on this review?</span>
      </div>
      <div class={css(styles.body)}>
        <span>
          {parts().join(" and ")} will be discarded. Notes and patches live only in this page, so
          there is nothing to restore them from.
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
          Clear {parts().join(" and ")}
        </button>
        <button type="button" class={css(styles.button, styles.spacer)} onClick={props.onClose}>
          Cancel
        </button>
      </div>
    </Modal>
  );
}

/**
 * `p` — the label for a patch that has just been drawn.
 *
 * The rectangle does not exist until this is answered: **Cancel removes it**,
 * which is the only way to discard a misdrawn selection and is why the dialog
 * offers no third way out — Esc and the backdrop both land on Cancel.
 *
 * A blank label is refused rather than defaulted. The label is the entire point
 * of a patch: a rectangle called "patch 3" tells a later reader nothing that the
 * coordinates do not, and since patches cannot be edited, a blank one could only
 * be fixed by drawing it again anyway.
 */
export function PatchDialog(props: {
  open: boolean;
  frame: NotedFrame | undefined;
  onCancel: () => void;
  onAdd: (label: string) => void;
}) {
  return (
    <Modal open={props.open} onClose={props.onCancel} label="Label this patch">
      <PatchForm frame={props.frame} onCancel={props.onCancel} onAdd={props.onAdd} />
    </Modal>
  );
}

/**
 * Mounted fresh with each opening — which is what empties the field.
 *
 * `Modal` renders its children only while it is open, so the state below is born
 * with the dialog and dies with it. Holding the draft in `PatchDialog` instead
 * would carry the previous patch's label into the next one.
 */
function PatchForm(props: {
  frame: NotedFrame | undefined;
  onCancel: () => void;
  onAdd: (label: string) => void;
}) {
  const [label, setLabel] = createSignal("");
  const ready = () => label().trim() !== "";
  const add = () => {
    if (ready()) props.onAdd(label().trim());
  };
  return (
    <>
      <div class={css(styles.head)}>
        <span class={css(styles.title)}>Patch</span>
        <span class={css(styles.sub)}>{props.frame?.label ?? "no frame"}</span>
      </div>
      <div class={css(styles.body)}>
        <input
          class={css(styles.input)}
          type="text"
          aria-label="Patch label"
          placeholder="cloud, white shirt, shadow under the bridge…"
          ref={(element) => queueMicrotask(() => element.focus())}
          onInput={(event) => setLabel(event.currentTarget.value)}
          onKeyDown={(event) => {
            // Enter is the gesture for a one-line field. Stopped rather than only
            // handled: this runs inside the dialog, and the page's own shortcuts
            // are watching the document.
            if (event.key !== "Enter") return;
            event.preventDefault();
            event.stopPropagation();
            add();
          }}
        />
      </div>
      <div class={css(styles.foot)}>
        <button
          type="button"
          class={css(styles.button, styles.primary)}
          disabled={!ready()}
          onClick={add}
        >
          Add patch
        </button>
        <span class={css(styles.status)}>
          <Show when={ready()} fallback="A patch needs a label — that is what it is for.">
            Enter adds it. Esc cancels and removes the rectangle.
          </Show>
        </span>
        <button type="button" class={css(styles.button, styles.spacer)} onClick={props.onCancel}>
          Cancel
        </button>
      </div>
    </>
  );
}
