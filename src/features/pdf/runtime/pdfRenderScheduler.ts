type ScheduledTask = {
  id: string;
  priority: number;
  sequence: number;
  controller: AbortController;
  run: (signal: AbortSignal) => Promise<void>;
  resolve: () => void;
  reject: (error: unknown) => void;
};

export type PdfScheduledRender = {
  promise: Promise<void>;
  cancel: () => void;
};

export class PdfRenderScheduler {
  private queue: ScheduledTask[] = [];
  private active: ScheduledTask | null = null;
  private sequence = 0;
  private disposed = false;

  schedule(id: string, priority: number, run: (signal: AbortSignal) => Promise<void>): PdfScheduledRender {
    this.cancel(id);
    const controller = new AbortController();
    let resolve!: () => void;
    let reject!: (error: unknown) => void;
    const promise = new Promise<void>((resolvePromise, rejectPromise) => {
      resolve = resolvePromise;
      reject = rejectPromise;
    });
    const task = { id, priority, sequence: this.sequence += 1, controller, run, resolve, reject };
    if (this.disposed) {
      controller.abort();
      resolve();
    } else {
      this.queue.push(task);
      this.queue.sort((left, right) => left.priority - right.priority || left.sequence - right.sequence);
      if (this.active && priority < this.active.priority) this.active.controller.abort();
      this.pump();
    }
    return { promise, cancel: () => this.cancel(id) };
  }

  cancel(id: string) {
    if (this.active?.id === id) this.active.controller.abort();
    const remaining: ScheduledTask[] = [];
    for (const task of this.queue) {
      if (task.id === id) {
        task.controller.abort();
        task.resolve();
      } else remaining.push(task);
    }
    this.queue = remaining;
  }

  dispose() {
    this.disposed = true;
    this.active?.controller.abort();
    for (const task of this.queue) {
      task.controller.abort();
      task.resolve();
    }
    this.queue = [];
  }

  private pump() {
    if (this.active || this.disposed) return;
    const task = this.queue.shift();
    if (!task) return;
    this.active = task;
    void task.run(task.controller.signal)
      .then(task.resolve, (error) => {
        if (task.controller.signal.aborted) task.resolve();
        else task.reject(error);
      })
      .finally(() => {
        if (this.active === task) this.active = null;
        this.pump();
      });
  }
}
