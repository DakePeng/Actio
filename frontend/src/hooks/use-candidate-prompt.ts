import { useCallback, useEffect, useRef, useState } from 'react';
import type { VoiceprintCandidate } from '../types/speaker';
import * as speakerApi from '../api/speakers';

const POLL_INTERVAL_MS = 5 * 60 * 1000; // 5 minutes

/**
 * Phase-C prompt driver. Polls `GET /candidates` on mount and periodically,
 * filters out candidates the user snoozed for this session, exposes the
 * top-priority candidate for the modal to render.
 *
 * "Snooze" is intentionally session-scoped — page reload clears it, so if
 * the same voice keeps showing up the user gets another chance to name it.
 * Permanent "not a person" goes through `dismissCandidate` which writes to
 * the DB and the cluster never resurfaces.
 */
export function useCandidatePrompt() {
  const [candidates, setCandidates] = useState<VoiceprintCandidate[]>([]);
  const snoozedRef = useRef<Set<string>>(new Set());
  const [snoozeVersion, setSnoozeVersion] = useState(0);

  const refresh = useCallback(async () => {
    try {
      const list = await speakerApi.listCandidates();
      setCandidates(list);
    } catch (e) {
      console.warn('[Actio] fetch candidates failed', e);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), POLL_INTERVAL_MS);
    return () => window.clearInterval(id);
  }, [refresh]);

  const activeCandidate =
    candidates.find((c) => !snoozedRef.current.has(c.candidate_id)) ?? null;

  const snooze = useCallback((candidateId: string) => {
    snoozedRef.current.add(candidateId);
    setSnoozeVersion((v) => v + 1);
  }, []);

  const confirm = useCallback(
    async (
      candidate: VoiceprintCandidate,
      input: { display_name: string; color: string },
    ) => {
      await speakerApi.confirmCandidate({
        display_name: input.display_name,
        color: input.color,
        member_segment_ids: candidate.member_segment_ids,
      });
      await refresh();
    },
    [refresh],
  );

  const dismiss = useCallback(
    async (candidate: VoiceprintCandidate) => {
      await speakerApi.dismissCandidate(candidate.member_segment_ids);
      await refresh();
    },
    [refresh],
  );

  // Include snoozeVersion in the return so consumers re-render when snooze changes.
  void snoozeVersion;

  return { activeCandidate, candidates, confirm, dismiss, snooze, refresh };
}
