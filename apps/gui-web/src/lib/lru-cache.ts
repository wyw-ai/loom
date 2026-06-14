/**
 * Generic LRU (Least Recently Used) cache with optional per-entry item caps.
 *
 * - Evicts the least-recently-accessed entry when maxEntries is exceeded.
 * - Each entry's value array is capped at maxItemsPerEntry (oldest items trimmed).
 * - Access (get/has) refreshes the entry's recency.
 */
export class LRUCache<K extends string, V> {
  private readonly maxEntries: number;
  private readonly maxItemsPerEntry: number;
  private readonly cache: Map<K, V[]>;
  private readonly accessOrder: K[];

  constructor(maxEntries: number, maxItemsPerEntry = Infinity) {
    this.maxEntries = maxEntries;
    this.maxItemsPerEntry = maxItemsPerEntry;
    this.cache = new Map();
    this.accessOrder = [];
  }

  /** Get a cached entry (refreshes recency). Returns a copy of the array. */
  get(key: K): V[] | undefined {
    const entry = this.cache.get(key);
    if (!entry) return undefined;
    this.touch(key);
    return [...entry];
  }

  /** Set a cached entry. Caps at maxItemsPerEntry, evicts LRU if over maxEntries. */
  set(key: K, items: V[]): void {
    const capped = this.maxItemsPerEntry < Infinity
      ? items.slice(-this.maxItemsPerEntry)
      : items;
    this.cache.set(key, capped);
    this.touch(key);
    this.evict();
  }

  /** Update an entry by applying an updater function. Returns the new array. */
  update(key: K, updater: (current: V[]) => V[]): V[] {
    const current = this.cache.get(key) ?? [];
    const updated = updater(current);
    const capped = this.maxItemsPerEntry < Infinity
      ? updated.slice(-this.maxItemsPerEntry)
      : updated;
    this.cache.set(key, capped);
    this.touch(key);
    this.evict();
    return [...capped];
  }

  /** Check if a key exists without affecting recency. */
  peek(key: K): V[] | undefined {
    const entry = this.cache.get(key);
    return entry ? [...entry] : undefined;
  }

  has(key: K): boolean {
    return this.cache.has(key);
  }

  /** Remove a specific entry. */
  delete(key: K): boolean {
    const removed = this.cache.delete(key);
    if (removed) {
      const idx = this.accessOrder.indexOf(key);
      if (idx >= 0) this.accessOrder.splice(idx, 1);
    }
    return removed;
  }

  /** Remove all entries. */
  clear(): void {
    this.cache.clear();
    this.accessOrder.length = 0;
  }

  get size(): number {
    return this.cache.size;
  }

  /** Snapshot of cache (for debugging). Does NOT affect recency. */
  snapshot(): Record<K, V[]> {
    const result = {} as Record<K, V[]>;
    for (const [key, value] of this.cache) {
      result[key] = [...value];
    }
    return result;
  }

  private touch(key: K): void {
    const idx = this.accessOrder.indexOf(key);
    if (idx >= 0) this.accessOrder.splice(idx, 1);
    this.accessOrder.push(key);
  }

  private evict(): void {
    while (this.cache.size > this.maxEntries) {
      const oldest = this.accessOrder.shift();
      if (oldest) this.cache.delete(oldest);
    }
  }
}
