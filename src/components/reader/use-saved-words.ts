'use client';

import * as React from 'react';
import { toast } from 'sonner';

import { api, errorMessage } from '@/hooks/use-api';
import type { DictionaryResult, SavedWord } from '@/types';

/**
 * The reader's word notebook.
 *
 * The dictionary itself is a server concern; this hook only keeps the list of
 * words the reader chose to keep, and updates it when one is saved from a lookup.
 */

export interface SavedWordsState {
  words: SavedWord[];
  loading: boolean;
  refresh: () => Promise<void>;
  save: (entry: DictionaryResult) => Promise<SavedWord | null>;
  remove: (id: string) => Promise<void>;
}

export function useSavedWords(enabled: boolean): SavedWordsState {
  const [words, setWords] = React.useState<SavedWord[]>([]);
  const [loading, setLoading] = React.useState(false);

  const refresh = React.useCallback(async () => {
    if (!enabled) return;
    setLoading(true);
    try {
      const result = await api.get<{ words: SavedWord[] }>('/api/dictionary/saved');
      setWords(result.words);
    } catch (error) {
      toast.error(errorMessage(error, 'Could not load your word list.'));
    } finally {
      setLoading(false);
    }
  }, [enabled]);

  React.useEffect(() => {
    void refresh();
  }, [refresh]);

  const save = React.useCallback(async (entry: DictionaryResult) => {
    try {
      const result = await api.post<{ word: SavedWord }>('/api/dictionary/saved', {
        word: entry.word,
        language: entry.language,
        phonetic: entry.phonetic ?? null,
        senses: entry.senses,
        sourceName: entry.source.name,
        sourceUrl: entry.source.url,
      });
      setWords((current) => {
        const rest = current.filter(
          (word) =>
            !(word.word === result.word.word && word.language === result.word.language),
        );
        return [result.word, ...rest];
      });
      toast.success(`“${result.word.word}” was added to your words.`);
      return result.word;
    } catch (error) {
      toast.error(errorMessage(error, 'Could not save that word.'));
      return null;
    }
  }, []);

  const remove = React.useCallback(async (id: string) => {
    let removed: SavedWord | undefined;
    setWords((current) => {
      removed = current.find((word) => word.id === id);
      return current.filter((word) => word.id !== id);
    });
    try {
      await api.delete(`/api/dictionary/saved/${id}`);
      toast.success('Word removed.');
    } catch (error) {
      if (removed) {
        const restored = removed;
        setWords((current) => [restored, ...current]);
      }
      toast.error(errorMessage(error, 'Could not remove that word.'));
    }
  }, []);

  return { words, loading, refresh, save, remove };
}
