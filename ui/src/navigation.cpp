#include "navigation.h"

#include <algorithm>

namespace Navigation
{

int Group::pages() const
{
    return (count + pageSize - 1) / pageSize;
}

int Group::rowsOn(int p) const
{
    return std::clamp(count - p * pageSize, 0, pageSize);
}

static void turnTo(Group &g, int page, int row)
{
    g.page = page;
    g.row = std::clamp(row, 0, g.rowsOn(page) - 1);
}

static bool down(State &s)
{
    Group &g = s.groups[s.current];
    const int rows = g.rowsOn(g.page), c = g.columns;
    if (g.row + c < rows)
        g.row += c;
    else if (g.row / c < (rows - 1) / c)
        g.row = rows - 1;
    else if (g.page + 1 < g.pages())
        turnTo(g, g.page + 1, g.row % c);
    else if (s.current == Docs && s.groups[Images].count > 0) {
        s.current = Images;
        s.groups[Images].row = 0;
    }
    return true;
}

static bool up(State &s)
{
    Group &g = s.groups[s.current];
    const int c = g.columns;
    if (g.row - c >= 0)
        g.row -= c;
    else if (g.page > 0) {
        const int rows = g.rowsOn(g.page - 1);
        turnTo(g, g.page - 1, (rows - 1) / c * c + g.row % c);
    } else if (s.current == Images && s.groups[Docs].count > 0) {
        Group &docs = s.groups[Docs];
        s.current = Docs;
        docs.row = docs.rowsOn(docs.page) - 1;
    }
    return true;
}

bool move(State &s, Key key)
{
    if (s.current == None) {
        if (key != Key::Down && key != Key::SwitchGroup)
            return false;
        s.current = s.groups[Docs].count > 0 ? Docs : s.groups[Images].count > 0 ? Images : None;
        return s.current != None;
    }
    Group &g = s.groups[s.current];
    switch (key) {
    case Key::Down:
        return down(s);
    case Key::Up:
        return up(s);
    case Key::Left:
    case Key::Right:
        if (g.columns == 1)
            return false;
        if (key == Key::Right) {
            if (g.row + 1 < g.rowsOn(g.page))
                ++g.row;
            else if (g.page + 1 < g.pages())
                turnTo(g, g.page + 1, 0);
        } else {
            if (g.row > 0)
                --g.row;
            else if (g.page > 0)
                turnTo(g, g.page - 1, g.pageSize - 1);
        }
        return true;
    case Key::PageDown:
        if (g.page + 1 < g.pages())
            turnTo(g, g.page + 1, g.row);
        return true;
    case Key::PageUp:
        if (g.page > 0)
            turnTo(g, g.page - 1, g.row);
        return true;
    case Key::SwitchGroup: {
        const int other = 1 - s.current;
        if (s.groups[other].count > 0) {
            s.current = other;
            Group &o = s.groups[other];
            o.row = std::clamp(o.row, 0, o.rowsOn(o.page) - 1);
        }
        return true;
    }
    }
    return false;
}

}
