/** Native directory inventory: POSIX directory handles are closed; no
 * application heap allocation. */
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

typedef struct {
    DIR *directory;
    size_t length;
    dev_t device;
    ino_t inode;
} TreeFrame;
typedef struct {
    TreeFrame frames[128];
    unsigned depth, entries;
    size_t bytes;
    char path[4097];
    Decoder *decoder;
} Tree;

/** Publish a bounded typed path record, framing symlink paths by length to
 * avoid ambiguity.
 * @param tree Inventory budget. @param kind Record type. @param target Optional
 * literal link target.
 */
static void tree_record(Tree *tree, char kind, const char *target) {
    assert(tree != NULL && tree->decoder != NULL);
    assert(strnlen(tree->path, sizeof(tree->path)) < sizeof(tree->path));
    char bytes[8300];
    int count = target ? snprintf(bytes, sizeof(bytes), "%c %zu:%s%s\n", kind, strlen(tree->path),
                                  tree->path, target)
                       : snprintf(bytes, sizeof(bytes), "%c %s\n", kind, tree->path);
    if (count < 0 || (size_t)count >= sizeof(bytes) || tree->bytes + (size_t)count > 33554432) {
        reject(tree->decoder, "directory inventory byte limit");
        return;
    }
    tree->bytes += (size_t)count;
    if (fwrite(bytes, 1, (size_t)count, stdout) != (size_t)count) {
        reject(tree->decoder, "directory inventory output");
    }
}

/** Follow directories while recording ancestor cycles and excessive depth
 * before pushing.
 * @param tree Inventory with room checked before modifying its fixed stack.
 */
static void tree_open(Tree *tree) {
    assert(tree != NULL);
    assert(tree->depth <= 128);
    if (tree->depth == 128) {
        reject(tree->decoder, "directory depth limit");
        return;
    }
    DIR *directory = opendir(tree->path);
    // A compiler under this effective identity cannot resolve descendants without search access.
    // Rechecking the marker on every lookup makes gained access invalidate the cached inventory.
    if (!directory && errno == EACCES &&
        faccessat(AT_FDCWD, tree->path, X_OK, AT_EACCESS) != 0 && errno == EACCES) {
        tree_record(tree, 'U', NULL);
        return;
    }
    struct stat status;
    if (!directory || fstat(dirfd(directory), &status) != 0) {
        if (directory) {
            (void)closedir(directory);
        }
        reject(tree->decoder, "directory open failed");
        return;
    }
    for (unsigned i = 0; i < tree->depth; i++) {
        if (tree->frames[i].device == status.st_dev && tree->frames[i].inode == status.st_ino) {
            (void)closedir(directory);
            tree_record(tree, 'C', NULL);
            return;
        }
    }
    tree->frames[tree->depth++] =
        (TreeFrame){directory, strlen(tree->path), status.st_dev, status.st_ino};
}

/** Inspect a node and record the literal symlink plus its followed type;
 * dangling links are explicit.
 * @param tree Bounded path. @param root Whether this node must be a directory
 * or missing root.
 */
static void tree_node(Tree *tree, bool root) {
    assert(tree != NULL);
    assert(tree->entries <= 131072);
    if (++tree->entries > 131072) {
        reject(tree->decoder, "directory entry limit");
        return;
    }
    struct stat status;
    if (lstat(tree->path, &status) != 0) {
        if (errno == ENOENT || errno == ENOTDIR) {
            tree_record(tree, 'M', NULL);
        } else {
            reject(tree->decoder, "directory stat failed");
        }
        return;
    }
    if (S_ISLNK(status.st_mode)) {
        char target[4097];
        ssize_t count = readlink(tree->path, target, sizeof(target));
        if (count < 0 || count == sizeof(target)) {
            reject(tree->decoder, "invalid symlink");
            return;
        }
        target[count] = 0;
        if (strchr(target, '\r') || strchr(target, '\n')) {
            reject(tree->decoder, "symlink control byte");
            return;
        }
        tree_record(tree, 'L', target);
        if (stat(tree->path, &status) != 0) {
            if (errno == ENOENT || errno == ENOTDIR) {
                tree_record(tree, 'M', NULL);
            } else {
                reject(tree->decoder, "symlink stat failed");
            }
            return;
        }
    }
    if (root && !S_ISDIR(status.st_mode)) {
        reject(tree->decoder, "non-directory search root");
        return;
    }
    tree_record(tree, S_ISDIR(status.st_mode) ? 'D' : S_ISREG(status.st_mode) ? 'F' : 'O', NULL);
    if (S_ISDIR(status.st_mode) && !tree->decoder->failed) {
        tree_open(tree);
    }
}

/** Visit one directory entry, validating its complete literal path before
 * filesystem work.
 * @param tree Nonempty stack; pop closes exactly one owned POSIX handle at EOF.
 */
static void tree_step(Tree *tree) {
    assert(tree != NULL);
    assert(tree->depth > 0 && tree->depth <= 128);
    TreeFrame *frame = &tree->frames[tree->depth - 1];
    errno = 0;
    struct dirent *entry = readdir(frame->directory);
    if (!entry) {
        int error = errno;
        if (closedir(frame->directory) != 0 || error != 0) {
            reject(tree->decoder, "directory read failed");
        }
        tree->depth--;
        return;
    }
    if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
        return;
    }
    size_t length = strnlen(entry->d_name, 4097);
    if (length == 4097 || frame->length + length + 1 > 4096 || strchr(entry->d_name, '\r') ||
        strchr(entry->d_name, '\n')) {
        reject(tree->decoder, "directory path limit/control byte");
        return;
    }
    tree->path[frame->length] = '/';
    memcpy(tree->path + frame->length + 1, entry->d_name, length + 1);
    tree_node(tree, false);
}

/** Decode at most 128 absolute search roots and traverse each within one
 * aggregate budget.
 * @param decoder Existing bounded input/output failure state. @param size Input
 * byte count.
 */
static void decode_tree(Decoder *decoder, size_t size) {
    assert(decoder != NULL);
    assert(size <= INPUT_LIMIT);
    Tree tree = {.decoder = decoder};
    if (size > 131072) {
        reject(decoder, "search root input limit");
        return;
    }
    unsigned roots = 0;
    for (size_t index = 0; index < size && !decoder->failed;) {
        size_t start = index;
        while (index < size && input[index] != '\n') {
            index++;
        }
        size_t length = index - start;
        if (++roots > 128 || index == size || length == 0 || length > 4096 || input[start] != '/') {
            reject(decoder, "invalid search roots");
            break;
        }
        memcpy(tree.path, input + start, length);
        tree.path[length] = 0;
        index++;
        tree_node(&tree, true);
        while (tree.depth > 0 && !decoder->failed) {
            tree_step(&tree);
        }
    }
    while (tree.depth > 0) {
        (void)closedir(tree.frames[--tree.depth].directory);
    }
}
