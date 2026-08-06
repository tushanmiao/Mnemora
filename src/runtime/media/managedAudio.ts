import { registerResource } from "../resources/ResourceRegistry";

export type ManagedAudio = {
  audio: HTMLAudioElement;
  release: () => void;
};

export function createManagedAudio(src: string, owner: string): ManagedAudio {
  const audio = new Audio(src);
  audio.preload = "none";
  let released = false;
  const releaseElement = () => {
    if (released) return;
    released = true;
    audio.pause();
    audio.removeAttribute("src");
    audio.load();
  };
  const registration = registerResource({
    owner,
    kind: "audio",
    backgroundReleasable: true,
    release: releaseElement,
  });
  return {
    audio,
    release() {
      registration.release();
      releaseElement();
    },
  };
}

