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
