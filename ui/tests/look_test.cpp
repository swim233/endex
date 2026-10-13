#include <QTest>

#include "look.h"

using namespace look;
using namespace Qt::StringLiterals;

class LookTest : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void accentContrast()
    {
        // A light custom accent is darkened until text in it reads on the view colour (4.5:1);
        // the ink on it is whichever of dark and white reads better.
        Settings s;
        s.preset = u"custom"_s;
        s.scheme = u"light"_s;
        s.accent = u"#27ae60"_s;
        const Look pale = resolve(s, {}, {}, false);
        QCOMPARE(pale.accent, QColor(u"#27ae60"_s));
        QVERIFY(contrast(pale.accentText, pale.view) >= 4.5);
        QVERIFY(pale.accentText != pale.accent);
        QCOMPARE(pale.accentInk, QColor(u"#0b1115"_s));
        QVERIFY(!pale.accentWarning);
        QCOMPARE(inkOn(QColor(u"#1d6b57"_s)), QColor(Qt::white));

        // An accent close to the window colour is flagged.
        s.accent = u"#f6e05e"_s;
        QVERIFY(resolve(s, {}, {}, false).accentWarning);

        // 跟随 Plasma: Plasma's colours untouched, no scheme to apply; an accent changes that.
        Settings plasma;
        QVERIFY(resolve(plasma, {}, {}, false).systemColors);
        plasma.accent = u"#da4453"_s; // ignored: the preset decides the accent
        QVERIFY(resolve(plasma, {}, {}, false).systemColors);
        plasma.scheme = u"dark"_s;
        const Look dark = resolve(plasma, {}, {}, false);
        QVERIFY(!dark.systemColors);
        QCOMPARE(dark.scheme, u"dark"_s);
        QCOMPARE(dark.window, QColor(u"#31363b"_s));
    }

    void popupSizeFitsTheScreen()
    {
        Look l;
        const QSize big(2560, 1440);
        auto on = [&l](QSize screen) {
            return popupSize(l, screen);
        };
        QCOMPARE(on(big).size, QSize(1100, 640));
        QVERIFY(!on(big).clamped);
        // 1366×768 still fits; 1280×720 leaves 600 px, a 800×600 screen 768×480.
        QCOMPARE(on({1366, 768}).size, QSize(1100, 640));
        QCOMPARE(on({1280, 720}).size, QSize(1100, 600));
        QVERIFY(on({1280, 720}).clamped);
        QCOMPARE(on({1280, 720}).wanted, QSize(1100, 640));
        QCOMPARE(on({800, 600}).size, QSize(768, 480));

        l.popupSize = u"screen"_s;
        l.screenPercent = 60;
        QCOMPARE(on({1920, 1080}).size, QSize(1152, 670));
        QCOMPARE(on(big).wanted, QSize(1400, 815));
        l.popupSize = u"custom"_s;
        l.popupWidth = 5000;
        l.popupHeight = 100;
        QCOMPARE(on(big).wanted, QSize(1600, 320));
    }

    void themeFile()
    {
        QString error;
        const auto file = ThemeFile::parse(uR"(# my theme
[theme]
scheme = "dark"   # always

[colors]
accent = "#E91E8C"
window = '#202020'

[font]
family = ""
size_pt = 11.5

[shape]
radius = 12
density = "compact"

[popup]
width = 1000
opacity = 0.8
blur = true
)"_s,
                                           &error);
        QVERIFY2(file, qPrintable(error));
        QCOMPARE(*file->scheme, u"dark"_s);
        QCOMPARE(file->colors.value(u"accent"_s), QColor(u"#e91e8c"_s));
        QCOMPARE(*file->family, QString());
        QCOMPARE(*file->sizePt, 11.5);
        QCOMPARE(*file->radius, 12);
        QCOMPARE(*file->width, 1000);
        QVERIFY(!file->height);
        QCOMPARE(file->keys.size(), 10);

        // The file wins over the page, and the colours in between follow its window colour.
        const Look l = resolve(Settings(), *file, {}, false);
        QCOMPARE(l.scheme, u"dark"_s);
        QCOMPARE(l.window, QColor(u"#202020"_s));
        QCOMPARE(l.accent, QColor(u"#e91e8c"_s));
        QVERIFY(!l.systemFont);
        QVERIFY(l.families.isEmpty());
        QCOMPARE(l.fontSize, 11.5);
        QCOMPARE(l.density, 0.72);
        QCOMPARE(l.popupSize, u"custom"_s);
        QCOMPARE(popupSize(l, {1920, 1080}).size, QSize(1000, 640));

        // The sample parses back, with quotes and backslashes in the family too.
        Look quoted;
        quoted.families = {uR"(My "Font" \ X)"_s};
        const auto sample = ThemeFile::parse(sampleToml(quoted, {1100, 640}), &error);
        QVERIFY2(sample, qPrintable(error));
        QCOMPARE(*sample->family, uR"(My "Font" \ X)"_s);

        const std::pair<QString, QString> broken[] = {
            {u"accent = \"#fff\""_s, u"第 1 行"_s},
            {u"[colour]\n"_s, u"没有 [colour]"_s},
            {u"[colors]\nacent = \"#ffffff\""_s, u"没有 acent"_s},
            {u"[colors]\naccent = \"blue\""_s, u"colors.accent 应为"_s},
            {u"[shape]\nradius = 40"_s, u"shape.radius 应为"_s},
            {u"[popup]\nblur = yes"_s, u"popup.blur"_s},
            {u"[font]\nfamily = \"Inter"_s, u"没有正确结束"_s},
            {u"[shape]\nradius = 4\nradius = 5"_s, u"第 3 行：shape.radius 写了两次"_s},
        };
        for (const auto &[text, message] : broken) {
            error.clear();
            QVERIFY2(!ThemeFile::parse(text, &error), qPrintable(text));
            QVERIFY2(error.contains(message), qPrintable(error));
        }
    }
};

QTEST_GUILESS_MAIN(LookTest)
#include "look_test.moc"
