#include <QTest>

#include "navigation.h"

using namespace Navigation;

class NavigationTest : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void pagesAndGroups()
    {
        // 20 documents (pages of 8: 8, 8, 4) and 11 images (pages of 9 in 3 columns: 9, 2).
        State s;
        s.groups[Docs] = {.count = 20, .pageSize = 8, .columns = 1};
        s.groups[Images] = {.count = 11, .pageSize = 9, .columns = 3};
        QVERIFY(!move(s, Key::Up));
        QVERIFY(move(s, Key::Down));
        QCOMPARE(s.current, int(Docs));
        QVERIFY(!move(s, Key::Right)); // the search field keeps ←/→ in the document list

        auto &docs = s.groups[Docs];
        docs.row = 7;
        move(s, Key::Down); // past the last row: next page
        QCOMPARE(docs.page, 1);
        QCOMPARE(docs.row, 0);
        move(s, Key::Up); // and back to the last row of the previous page
        QCOMPARE(docs.page, 0);
        QCOMPARE(docs.row, 7);
        docs.row = 6;
        move(s, Key::PageDown);
        move(s, Key::PageDown); // the short last page clamps the row
        QCOMPARE(docs.page, 2);
        QCOMPARE(docs.row, 3);
        move(s, Key::PageDown);
        QCOMPARE(docs.page, 2);
        move(s, Key::Down); // past the last document: into the images
        QCOMPARE(s.current, int(Images));

        auto &images = s.groups[Images];
        QCOMPARE(images.row, 0);
        move(s, Key::Down);
        QCOMPARE(images.row, 3);
        images.row = 8;
        move(s, Key::Down); // next page, same column, clamped to the two images there
        QCOMPARE(images.page, 1);
        QCOMPARE(images.row, 1);
        move(s, Key::Up); // previous page, last grid row, same column
        QCOMPARE(images.page, 0);
        QCOMPARE(images.row, 7);
        images.row = 8;
        move(s, Key::Right);
        QCOMPARE(images.page, 1);
        QCOMPARE(images.row, 0);
        move(s, Key::Left);
        QCOMPARE(images.page, 0);
        QCOMPARE(images.row, 8);
        images.row = 1;
        move(s, Key::Up); // top grid row of the first page: back to the documents
        QCOMPARE(s.current, int(Docs));
        QCOMPARE(docs.row, 3);

        move(s, Key::SwitchGroup);
        QCOMPARE(s.current, int(Images));
        s.groups[Docs].count = 0;
        move(s, Key::SwitchGroup); // nothing to switch to
        QCOMPARE(s.current, int(Images));
    }
};

QTEST_GUILESS_MAIN(NavigationTest)
#include "navigation_test.moc"
