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
}

export function buildTree(entries: ArchiveEntry[]): TreeNode {
  const root: TreeNode = { name: '', path: '', isDir: true, size: 0, children: [] };
  const index = new Map<string, TreeNode>();
  index.set('', root);

  const sorted = [...entries].sort((a, b) => a.path.localeCompare(b.path));

  for (const entry of sorted) {
    const parts = entry.path.split('/').filter(Boolean);
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
        };
        index.set(fullPath, node);
        const parent = index.get(parentPath)!;
        parent.children.push(node);
      } else if (isLast) {
        node.isDir = entry.type === 'd';
        if (entry.type === 'f') node.size = entry.size;
      }

      parentPath = fullPath;
    }
  }

  return root;
}

export function collectFilePaths(node: TreeNode): string[] {
  if (!node.isDir) return [node.path];
  const out: string[] = [];
  for (const child of node.children) {
    out.push(...collectFilePaths(child));
  }
  return out;
}

export interface FolderState {
  total: number;
  selected: number;
}

export function folderState(node: TreeNode, selected: ReadonlySet<string>): FolderState {
  const leaves = collectFilePaths(node);
  let count = 0;
  for (const p of leaves) {
    if (selected.has(p)) count++;
  }
  return { total: leaves.length, selected: count };
}
