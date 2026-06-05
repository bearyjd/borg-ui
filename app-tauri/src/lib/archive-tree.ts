export interface ArchiveEntry {
  path: string;
  mode: string;
  size: number;
  type: string;
}

export interface TreeNode {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  children: TreeNode[];
  /** Cached count of file (non-dir) descendants. Populated after buildTree. */
  leafCount: number;
  /** Cached file paths for selection batching. Populated after buildTree. */
  leafPaths: string[];
}

/** Convert any backslash separators to forward slashes — borg's marcpope port
 *  uses `/`, but a future native build could emit `\`. */
function normalizePath(p: string): string {
  return p.includes('\\') ? p.replace(/\\/g, '/') : p;
}

export function buildTree(entries: ArchiveEntry[]): TreeNode {
  const root: TreeNode = {
    name: '',
    path: '',
    isDir: true,
    size: 0,
    children: [],
    leafCount: 0,
    leafPaths: [],
  };
  const index = new Map<string, TreeNode>();
  index.set('', root);

  const sorted = [...entries].sort((a, b) => a.path.localeCompare(b.path));

  for (const entry of sorted) {
    const parts = normalizePath(entry.path).split('/').filter(Boolean);
    if (parts.length === 0) continue;

    let parentPath = '';
    for (let i = 0; i < parts.length; i++) {
      const name = parts[i];
      const fullPath = parentPath ? `${parentPath}/${name}` : name;
      const isLast = i === parts.length - 1;

      let node = index.get(fullPath);
      if (!node) {
        node = {
          name,
          path: fullPath,
          isDir: isLast ? entry.type === 'd' : true,
          size: isLast && entry.type === 'f' ? entry.size : 0,
          children: [],
          leafCount: 0,
          leafPaths: [],
        };
        index.set(fullPath, node);
        const parent = index.get(parentPath);
        if (!parent) {
          throw new Error(`buildTree invariant: missing parent for "${fullPath}"`);
        }
        parent.children.push(node);
      } else if (isLast) {
        node.isDir = entry.type === 'd';
        if (entry.type === 'f') node.size = entry.size;
      }

      parentPath = fullPath;
    }
  }

  populateLeafCache(root);
  return root;
}

/** Post-order walk that caches leafCount + leafPaths on every node. */
function populateLeafCache(node: TreeNode): void {
  if (!node.isDir) {
    node.leafCount = 1;
    node.leafPaths = [node.path];
    return;
  }
  const paths: string[] = [];
  for (const child of node.children) {
    populateLeafCache(child);
    paths.push(...child.leafPaths);
  }
  node.leafCount = paths.length;
  node.leafPaths = paths;
}

/** Returns cached file paths for this subtree (no traversal). */
export function collectFilePaths(node: TreeNode): string[] {
  return node.leafPaths;
}

export interface FolderState {
  total: number;
  selected: number;
}

export function folderState(node: TreeNode, selected: ReadonlySet<string>): FolderState {
  let count = 0;
  for (const p of node.leafPaths) {
    if (selected.has(p)) count++;
  }
  return { total: node.leafCount, selected: count };
}

export interface VisibleRow {
  node: TreeNode;
  depth: number;
}

/** Depth-first flatten of the tree into the linear list of rows the virtual
 *  scroller windows over. A directory's children are included only when its
 *  path is in `expanded`. The root node itself is not emitted — its children
 *  are the top level — matching the previous `tree.children` render entry. */
export function flattenVisible(root: TreeNode, expanded: ReadonlySet<string>): VisibleRow[] {
  const rows: VisibleRow[] = [];
  const walk = (node: TreeNode, depth: number): void => {
    for (const child of node.children) {
      rows.push({ node: child, depth });
      if (child.isDir && expanded.has(child.path)) {
        walk(child, depth + 1);
      }
    }
  };
  walk(root, 0);
  return rows;
}

/** Selection state for the archive browser. The set of selected *file* (leaf)
 *  paths is the source of truth handed to restore; alongside it we keep an
 *  incrementally-maintained per-directory count of selected descendants, so a
 *  folder's checked/indeterminate state is O(1) to read instead of re-walking
 *  its subtree on every render. Toggling a single file is O(path depth);
 *  toggling a folder or "select all" is O(affected leaves), as it must be. */
export class Selection {
  private leaves = new Set<string>();
  /** dirPath -> number of selected leaf descendants. Absent key means zero. */
  private dirSelected = new Map<string, number>();

  constructor(private readonly root: TreeNode) {}

  get size(): number {
    return this.leaves.size;
  }

  selectedPaths(): string[] {
    return [...this.leaves];
  }

  isSelected(path: string): boolean {
    return this.leaves.has(path);
  }

  /** Number of selected leaf descendants under a directory node (0 if none). */
  selectedUnder(node: TreeNode): number {
    return this.dirSelected.get(node.path) ?? 0;
  }

  /** Directory paths containing `leafPath`, from the root ('') down to the
   *  immediate parent. The leaf itself is never included. */
  private static ancestorDirs(leafPath: string): string[] {
    const dirs: string[] = [''];
    let slash = leafPath.indexOf('/');
    while (slash >= 0) {
      dirs.push(leafPath.slice(0, slash));
      slash = leafPath.indexOf('/', slash + 1);
    }
    return dirs;
  }

  private addLeaf(path: string): void {
    if (this.leaves.has(path)) return;
    this.leaves.add(path);
    for (const dir of Selection.ancestorDirs(path)) {
      this.dirSelected.set(dir, (this.dirSelected.get(dir) ?? 0) + 1);
    }
  }

  private removeLeaf(path: string): void {
    if (!this.leaves.has(path)) return;
    this.leaves.delete(path);
    for (const dir of Selection.ancestorDirs(path)) {
      const next = (this.dirSelected.get(dir) ?? 0) - 1;
      if (next <= 0) this.dirSelected.delete(dir);
      else this.dirSelected.set(dir, next);
    }
  }

  /** Toggle a node. A file flips itself; a directory selects all of its files,
   *  or clears them if it is already fully selected. */
  toggle(node: TreeNode): void {
    if (!node.isDir) {
      if (this.leaves.has(node.path)) this.removeLeaf(node.path);
      else this.addLeaf(node.path);
      return;
    }
    const leaves = node.leafPaths;
    const allSelected = leaves.length > 0 && this.selectedUnder(node) === leaves.length;
    if (allSelected) {
      for (const p of leaves) this.removeLeaf(p);
    } else {
      for (const p of leaves) this.addLeaf(p);
    }
  }

  selectAll(): void {
    // Every file selected ⇒ every directory's count equals its leafCount, so
    // skip the per-leaf ancestor walk and set counts directly.
    this.leaves = new Set(this.root.leafPaths);
    this.dirSelected = new Map();
    const walk = (node: TreeNode): void => {
      if (!node.isDir) return;
      // Keep the "absent key means zero" invariant: empty dirs hold no count.
      if (node.leafCount > 0) this.dirSelected.set(node.path, node.leafCount);
      for (const child of node.children) walk(child);
    };
    walk(this.root);
  }

  clear(): void {
    this.leaves = new Set();
    this.dirSelected = new Map();
  }
}
