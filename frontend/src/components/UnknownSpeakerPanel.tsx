import { useEffect, useState } from 'react';
import { useVoiceStore } from '../store/use-voice-store';
import { AssignSpeakerPicker } from './AssignSpeakerPicker';

export function UnknownSpeakerPanel() {
  const unknowns = useVoiceStore((s) => s.unknowns);
  const dismissed = useVoiceStore((s) => s.dismissedUnknowns);
  const fetchUnknowns = useVoiceStore((s) => s.fetchUnknowns);
  const assignSegment = useVoiceStore((s) => s.assignSegment);
  const dismissUnknown = useVoiceStore((s) => s.dismissUnknown);
  const [pickingFor, setPickingFor] = useState<string | null>(null);

  useEffect(() => {
    void fetchUnknowns();
    const interval = window.setInterval(() => {
      void fetchUnknowns();
    }, 10_000);
    return () => window.clearInterval(interval);
  }, [fetchUnknowns]);

  const visible = unknowns.filter((u) => !dismissed.has(u.segment_id));
  if (visible.length === 0) return null;

  return (
    <details className="unknown-panel" open>
      <summary>Unidentified voices ({visible.length})</summary>
      <ul className="unknown-panel__list">
        {visible.map((u) => (
          <li key={u.segment_id} className="unknown-panel__row">
            <div className="unknown-panel__meta">
              {((u.end_ms - u.start_ms) / 1000).toFixed(1)}s · session{' '}
              {u.session_id.slice(0, 8)}…
              {!u.has_embedding && ' · no voiceprint from this clip'}
            </div>
            {pickingFor === u.segment_id ? (
              <AssignSpeakerPicker
                onPick={async (target) => {
                  try {
                    await assignSegment(u.segment_id, target);
                  } catch (e) {
                    console.warn('[Actio] assign segment failed', e);
                  }
                  setPickingFor(null);
                }}
                onCancel={() => setPickingFor(null)}
              />
            ) : (
              <div className="unknown-panel__actions">
                <button
                  type="button"
                  className="primary-button"
                  onClick={() => setPickingFor(u.segment_id)}
                >
                  Assign to…
                </button>
                <button
                  type="button"
                  className="secondary-button"
                  onClick={() => dismissUnknown(u.segment_id)}
                >
                  Not a person
                </button>
              </div>
            )}
          </li>
        ))}
      </ul>
    </details>
  );
}
