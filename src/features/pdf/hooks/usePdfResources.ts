import { useCallback, useEffect, useRef, useState } from "react";
import {
  createLibraryAnnotation,
  createLibraryNote,
  deleteLibraryAnnotation,
  deleteLibraryNote,
  listLibraryAnnotations,
  listLibraryNotes,
  updateLibraryAnnotation,
  updateLibraryNote,
} from "../../library/api/library";
import type {
  LibraryAnnotation,
  LibraryAnnotationColor,
  LibraryAnnotationKind,
  LibraryAnnotationRect,
  LibraryNote,
  LibraryNoteSummary,
} from "../../library/types";

/** PDF 关联的批注与笔记按文献加载，离开文献后立即使未完成请求失效。 */
export function usePdfResources(itemId: string) {
  const generationRef = useRef(0);
  const notesLoadRef = useRef({ itemId: "", loading: false, loaded: false });
  const annotationCreateRef = useRef(false);
  const [annotations, setAnnotations] = useState<LibraryAnnotation[]>([]);
  const [notes, setNotes] = useState<LibraryNoteSummary[]>([]);
  const [annotationsLoading, setAnnotationsLoading] = useState(false);
  const [notesLoading, setNotesLoading] = useState(false);
  const [notesLoaded, setNotesLoaded] = useState(false);
  const [annotationError, setAnnotationError] = useState("");
  const [noteError, setNoteError] = useState("");

  useEffect(() => {
    const generation = generationRef.current + 1;
    generationRef.current = generation;
    setAnnotations([]);
    setNotes([]);
    setAnnotationError("");
    setNoteError("");
    setAnnotationsLoading(true);
    setNotesLoading(false);
    setNotesLoaded(false);
    notesLoadRef.current = { itemId, loading: false, loaded: false };
    void listLibraryAnnotations(itemId)
      .then((next) => {
        if (generationRef.current !== generation) return;
        setAnnotations((current) => {
          const merged = new Map(next.map((annotation) => [annotation.id, annotation]));
          for (const annotation of current) merged.set(annotation.id, annotation);
          return sortAnnotations([...merged.values()]);
        });
      })
      .catch((error) => {
        if (generationRef.current === generation) setAnnotationError(errorMessage(error));
      })
      .finally(() => {
        if (generationRef.current === generation) setAnnotationsLoading(false);
      });
    return () => {
      generationRef.current += 1;
      if (notesLoadRef.current.itemId === itemId) {
        notesLoadRef.current = { itemId: "", loading: false, loaded: false };
      }
    };
  }, [itemId]);

  const createAnnotation = useCallback(async (
    kind: LibraryAnnotationKind,
    pageIndex: number,
    color: LibraryAnnotationColor,
    text: string,
    rects: LibraryAnnotationRect[],
  ) => {
    if (annotationCreateRef.current) return null;
    annotationCreateRef.current = true;
    setAnnotationError("");
    try {
      const annotation = await createLibraryAnnotation({ itemId, kind, pageIndex, color, text, rects });
      setAnnotations((current) => sortAnnotations([...current, annotation]));
      return annotation;
    } catch (error) {
      setAnnotationError(errorMessage(error));
      return null;
    } finally {
      annotationCreateRef.current = false;
    }
  }, [itemId]);

  const updateAnnotation = useCallback(async (annotationId: string, color: LibraryAnnotationColor, comment: string) => {
    setAnnotationError("");
    try {
      const annotation = await updateLibraryAnnotation({ annotationId, color, comment });
      setAnnotations((current) => current.map((candidate) => candidate.id === annotation.id ? annotation : candidate));
      return annotation;
    } catch (error) {
      setAnnotationError(errorMessage(error));
      throw error;
    }
  }, []);

  const deleteAnnotation = useCallback(async (annotationId: string) => {
    setAnnotationError("");
    try {
      const removed = await deleteLibraryAnnotation(annotationId);
      if (removed) setAnnotations((current) => current.filter((item) => item.id !== annotationId));
      return removed;
    } catch (error) {
      setAnnotationError(errorMessage(error));
      throw error;
    }
  }, []);

  const loadNotes = useCallback(async () => {
    const current = notesLoadRef.current;
    if (current.itemId === itemId && (current.loading || current.loaded)) return;
    notesLoadRef.current = { itemId, loading: true, loaded: false };
    setNotesLoading(true);
    setNoteError("");
    try {
      const next = await listLibraryNotes(itemId);
      if (notesLoadRef.current.itemId !== itemId) return;
      notesLoadRef.current = { itemId, loading: false, loaded: true };
      setNotes(next);
      setNotesLoaded(true);
    } catch (error) {
      if (notesLoadRef.current.itemId !== itemId) return;
      notesLoadRef.current = { itemId, loading: false, loaded: false };
      setNoteError(errorMessage(error));
    } finally {
      if (notesLoadRef.current.itemId === itemId) setNotesLoading(false);
    }
  }, [itemId]);

  const createNote = useCallback(async (title: string, content: string) => {
    setNoteError("");
    try {
      const note = await createLibraryNote({ itemId, title, content });
      notesLoadRef.current = { itemId, loading: false, loaded: true };
      setNotesLoaded(true);
      setNotes((current) => [noteSummary(note), ...current]);
      return note;
    } catch (error) {
      setNoteError(errorMessage(error));
      throw error;
    }
  }, [itemId]);

  const updateNote = useCallback(async (noteId: string, title: string, content: string) => {
    setNoteError("");
    try {
      const note = await updateLibraryNote({ noteId, title, content });
      setNotes((current) => current.map((candidate) => candidate.id === note.id ? noteSummary(note) : candidate));
      return note;
    } catch (error) {
      setNoteError(errorMessage(error));
      throw error;
    }
  }, []);

  const deleteNote = useCallback(async (noteId: string) => {
    setNoteError("");
    try {
      const removed = await deleteLibraryNote(noteId);
      if (removed) setNotes((current) => current.filter((note) => note.id !== noteId));
      return removed;
    } catch (error) {
      setNoteError(errorMessage(error));
      throw error;
    }
  }, []);

  return {
    annotations,
    notes,
    annotationsLoading,
    notesLoading,
    notesLoaded,
    annotationError,
    noteError,
    setAnnotationError,
    createAnnotation,
    updateAnnotation,
    deleteAnnotation,
    loadNotes,
    createNote,
    updateNote,
    deleteNote,
  };
}

function noteSummary(note: LibraryNote): LibraryNoteSummary {
  return {
    id: note.id,
    itemId: note.itemId,
    itemTitle: note.itemTitle,
    title: note.title,
    contentPreview: note.content.slice(0, 600),
    contentChars: note.content.length,
    groupName: note.groupName,
    createdAt: note.createdAt,
    updatedAt: note.updatedAt,
  };
}

function sortAnnotations(annotations: LibraryAnnotation[]) {
  return [...annotations].sort((left, right) => left.pageIndex - right.pageIndex || left.createdAt - right.createdAt);
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
