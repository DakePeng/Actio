import { useCallback, useEffect, useState } from 'react';
import type { VoiceprintCandidate } from '../types/speaker';
import * as speakerApi from '../api/speakers';

const POLL_INTERVAL_MS = 2 * 60 * 1000; // 2 minutes

/**
 * Polls `GET /candidates` and exposes confirm/dismiss actions. The People
 * tab renders the returned list as a "Pending voices" section — the user
 * chooses when to deal with each candidate rather than being interrupted
 * by a modal.
 */
export function useCandidatePrompt() {
  const [candidates, setCandidates] = useState<VoiceprintCandidate[]>([]);

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

  return { candidates, confirm, dismiss, refresh };
}
