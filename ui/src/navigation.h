// Keyboard movement over the two result groups, which page independently: documents (one
// column) and images (a grid).

#pragma once

namespace Navigation
{

enum class Key { Up, Down, Left, Right, PageUp, PageDown, SwitchGroup };

struct Group {
    int count = 0;
    int pageSize = 1;
    int columns = 1;
    int page = 0;
    // Index of the selected item on the current page.
    int row = 0;

    int pages() const;
    int rowsOn(int page) const;
};

enum { Docs, Images, None = -1 };

struct State {
    Group groups[2];
    int current = None;
};

// Applies `key`; returns false when the key means nothing here (e.g. ← in the document list),
// so that the search field can use it.
bool move(State &state, Key key);

}
