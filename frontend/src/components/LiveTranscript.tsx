import { useMemo } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { useVoiceStore } from '../store/use-voice-store';
import type { TranscriptLine, TranslationEntry } from '../store/use-voice-store';
import type { Speaker } from '../types/speaker';
import { useT } from '../i18n';

interface Bubble {
  key: string;
  speakerId: string | null;
  resolved: boolean;
  lines: TranscriptLine[];
}

/** Group consecutive same-speaker lines into bubbles. Unresolved lines
 *  (speaker_id === null, resolved === false) cluster into an "Identifying…"
 *  group; resolved-but-unmatched lines cluster into a separate "Unknown"
 *  group — merging them would let a later Unknown event silently relabel
 *  a still-identifying bubble. */
function groupLines(lines: TranscriptLine[]): Bubble[] {
  const bubbles: Bubble[] = [];
  for (const line of lines) {
    const last = bubbles[bubbles.length - 1];
    if (
      last &&
      last.speakerId === line.speaker_id &&
      last.resolved === line.resolved
    ) {
      last.lines.push(line);
    } else {
      bubbles.push({
        key: line.id,
        speakerId: line.speaker_id,
        resolved: line.resolved,
        lines: [line],
      });
    }
  }
  return bubbles;
}

function SpeakerHeader({
  speakerId,
  resolved,
  speakers,
}: {
  speakerId: string | null;
  resolved: boolean;
  speakers: Speaker[];
}) {
  const t = useT();
  if (speakerId === null && !resolved) {
    return (
      <div className="live-transcript__header live-transcript__header--unresolved">
        <motion.span
          className="live-transcript__avatar live-transcript__avatar--unresolved"
          aria-hidden="true"
          animate={{ opacity: [0.5, 1, 0.5] }}
          transition={{ duration: 1.4, repeat: Infinity, ease: 'easeInOut' }}
        >
          …
        </motion.span>
        <span className="live-transcript__name">{t('transcript.identifying')}</span>
      </div>
    );
  }
  if (speakerId === null) {
    return (
      <div className="live-transcript__header live-transcript__header--unresolved">
        <span
          className="live-transcript__avatar live-transcript__avatar--unresolved"
          aria-hidden="true"
        >
          ?
        </span>
        <span className="live-transcript__name">{t('transcript.unknown')}</span>
      </div>
    );
  }
  const speaker = speakers.find((s) => s.id === speakerId);
  const name = speaker?.display_name ?? 'Unknown';
  const color = speaker?.color ?? '#9ca3af';
  return (
    <div className="live-transcript__header">
      <span
        className="live-transcript__avatar"
        style={{ backgroundColor: color }}
        aria-hidden="true"
      >
        {name.charAt(0).toUpperCase()}
      </span>
      <span className="live-transcript__name" style={{ color }}>
        {name}
      </span>
    </div>
  );
}

/** A chunk of consecutive lines whose translation state is being
 *  rendered together. Pending/error chunks collapse into a single
 *  status indicator (no point in showing 4 spinners side-by-side
 *  for 4 short lines on the same visual row). 'done' lines remain
 *  individual since each translation is distinct content. */
type Chunk =
  | { kind: 'individual'; line: TranscriptLine }
  | { kind: 'group'; status: 'pending' | 'error'; lines: TranscriptLine[] };

function chunkBubbleLines(
  lines: TranscriptLine[],
  byLineId: Record<string, TranslationEntry | undefined>,
  enabled: boolean,
): Chunk[] {
  const chunks: Chunk[] = [];
  for (const line of lines) {
    const entry = byLineId[line.id];
    const status = entry?.status;
    const groupable: 'pending' | 'error' | null =
      enabled && (status === 'pending' || status === 'error') ? status : null;
    if (groupable === null) {
      chunks.push({ kind: 'individual', line });
      continue;
    }
    const last = chunks[chunks.length - 1];
    if (last && last.kind === 'group' && last.status === groupable) {
      last.lines.push(line);
    } else {
      chunks.push({ kind: 'group', status: groupable, lines: [line] });
    }
  }
  return chunks;
}

function TranscriptLineRow({ line }: { line: TranscriptLine }) {
  const enabled = useVoiceStore((s) => s.translation.enabled);
  const entry = useVoiceStore((s) => s.translation.byLineId[line.id]);

  // The translation prompt instructs the LLM to echo the source verbatim
  // when it's already in the target language. Rendering the same text
  // twice is just visual noise — suppress the annotation in that case.
  const isPassthrough =
    entry?.status === 'done' &&
    entry.text !== undefined &&
    entry.text.trim() === line.text.trim();

  return (
    <span className="live-transcript__line">
      <span className="live-transcript__line-text">{line.text} </span>
      {enabled && entry?.status === 'done' && !isPassthrough && (
        <span className="live-transcript__translation live-transcript__translation--done">
          {entry.text}
        </span>
      )}
    </span>
  );
}

/** One indicator for a run of consecutive pending/error lines. The
 *  source lines flow inline as a single block of text; the status
 *  indicator sits underneath spanning the whole block. */
function ChunkGroup({
  status,
  lines,
}: {
  status: 'pending' | 'error';
  lines: TranscriptLine[];
}) {
  const t = useT();
  const retry = useVoiceStore((s) => s.retryTranslationLine);
  const onRetry = () => {
    for (const l of lines) retry(l.id);
  };
  return (
    <span className="live-transcript__chunk">
      <span className="live-transcript__line-text">
        {lines.map((l) => `${l.text} `).join('')}
      </span>
      <span className={`live-transcript__translation live-transcript__translation--${status}`}>
        {status === 'pending' && t('transcript.translating')}
        {status === 'error' && (
          <button
            type="button"
            className="live-transcript__translation-retry"
            onClick={onRetry}
          >
            ⚠ {t('transcript.translateError')}
          </button>
        )}
      </span>
    </span>
  );
}

/** Renders the current session's finalized lines as speaker-grouped bubbles,
 *  with the pending partial trailing as italic text under its own
 *  "Identifying…" header. */
export function LiveTranscript({
  lines,
  pendingPartial,
}: {
  lines: TranscriptLine[];
  pendingPartial: TranscriptLine | null;
}) {
  const speakers = useVoiceStore((s) => s.speakers);
  const translationEnabled = useVoiceStore((s) => s.translation.enabled);
  const byLineId = useVoiceStore((s) => s.translation.byLineId);
  const t = useT();
  const bubbles = useMemo(() => groupLines(lines), [lines]);

  // If a partial is in flight and its speaker matches the last bubble's
  // speaker (only really possible when the partial has no speaker AND the
  // last bubble is also unresolved), glue it to that bubble. Otherwise
  // render a separate unresolved bubble at the bottom.
  const lastBubble = bubbles[bubbles.length - 1];
  const partialFitsLast =
    pendingPartial !== null &&
    lastBubble !== undefined &&
    lastBubble.speakerId === pendingPartial.speaker_id &&
    lastBubble.resolved === pendingPartial.resolved;

  return (
    <div className="live-transcript">
      <AnimatePresence initial={false}>
        {bubbles.map((b, i) => {
          const isLast = i === bubbles.length - 1;
          const attachedPartial = isLast && partialFitsLast ? pendingPartial : null;
          return (
            <motion.div
              key={b.key}
              className="live-transcript__bubble"
              layout
              initial={{ opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, transition: { duration: 0.1 } }}
              transition={{ type: 'spring', stiffness: 320, damping: 28 }}
            >
              <SpeakerHeader
                speakerId={b.speakerId}
                resolved={b.resolved}
                speakers={speakers}
              />
              <div className="live-transcript__body">
                {chunkBubbleLines(b.lines, byLineId, translationEnabled).map(
                  (chunk, idx) => {
                    if (chunk.kind === 'individual') {
                      return <TranscriptLineRow key={chunk.line.id} line={chunk.line} />;
                    }
                    return (
                      <ChunkGroup
                        key={`chunk-${idx}-${chunk.lines[0]!.id}`}
                        status={chunk.status}
                        lines={chunk.lines}
                      />
                    );
                  },
                )}
                {attachedPartial && (
                  <span className="live-transcript__partial">
                    {' '}
                    {attachedPartial.text}
                  </span>
                )}
              </div>
            </motion.div>
          );
        })}

        {/* Standalone partial bubble when it doesn't attach to the last one */}
        {pendingPartial && !partialFitsLast && (
          <motion.div
            key="partial"
            className="live-transcript__bubble live-transcript__bubble--partial"
            layout
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, transition: { duration: 0.1 } }}
          >
            <SpeakerHeader
              speakerId={pendingPartial.speaker_id}
              resolved={pendingPartial.resolved}
              speakers={speakers}
            />
            <div className="live-transcript__body">
              <span className="live-transcript__partial">
                {pendingPartial.text}
              </span>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {bubbles.length === 0 && !pendingPartial && (
        <span className="live-tab__transcript-placeholder">
          {t('recording.listening')}
        </span>
      )}
    </div>
  );
}
