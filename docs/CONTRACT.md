# قرارداد پروژه PersianCalendar (برای هماهنگی تیم)

این سند مرجع مشترک Lead، تیم Rust و تیم فرانت‌اند است. هر نام/شکل داده که اینجا آمده
«قرارداد» است و نباید بدون هماهنگی عوض شود.

## استک

- Tauri v2 (Rust) + Vite + Vanilla TypeScript
- تبدیل تاریخ در فرانت: `jalaali-js`
- دریافت مناسبت‌ها: `reqwest` با `rustls-tls` در Rust، کش JSON در `app_cache_dir/events/`
- پلاگین‌ها: `single-instance`, `autostart`, `positioner` (با feature `tray-icon`), `store`, `clipboard-manager`
- فونت: Vazirmatn به‌صورت محلی (در `src/assets/fonts/*.woff2` و `src-tauri/assets/*.ttf`)

## پنجره‌ها (Labels)

| label | توضیح |
|---|---|
| `popup` | پاپ‌آپ تقویم؛ در `tauri.conf.json` تعریف شده؛ `visible: false`، بدون دکوراسیون، شفاف، alwaysOnTop، skipTaskbar، غیرقابل resize |
| `widget` | ویجت دسکتاپ؛ در زمان اجرا توسط Rust ساخته می‌شود؛ شفاف، بدون دکوراسیون، alwaysOnBottom، skipTaskbar |

ورودی‌ها: `index.html` → `src/main.ts` (popup) و `widget.html` → `src/widget-main.ts` (widget).
هر دو از `getCurrentWindow().label` استفاده می‌کنند.

## تنظیمات (Settings) — شکل JSON کانونیک

ذخیره با `tauri-plugin-store` در فایل `settings.json` (کلید `settings`).
فرانت‌اند هرگز مستقیم فایل را نمی‌نویسد؛ فقط از طریق دستورهای زیر.

```jsonc
{
  "theme": "auto",             // "auto" | "light" | "dark"
  "accent": "#2f6fed",         // #rrggbb
  "digits": "fa",              // "fa" | "en"
  "font": "vazirmatn",         // "vazirmatn" | "system"
  "trayIconMode": "day",       // "day" | "static"
  "weekStart": 0,              // 0=شنبه، 1=یکشنبه، 2=دوشنبه
  "showGregorian": false,
  "showLunar": false,
  "lunarOffset": 0,            // -3..3؛ جابه‌جایی لایه‌ی قمری (تاریخ/مناسبت/تعطیلات)
  "showEvents": true,
  "eventsMode": "important",   // "important" | "all"
  "showWeekNumber": false,
  "copyFormat": "jalaliSlash", // "jalaliSlash" | "jalaliLong" | "gregorianIso"
  "autostart": false,
  "closeOnBlur": true,
  "widgetEnabled": false,
  "widgetSize": "full",        // "small" | "full"
  "widgetOpacity": 92,         // 30..100
  "widgetLocked": false,
  "widgetX": null,             // i32 | null
  "widgetY": null
}
```

ذخیره خودکار: فرانت‌اند بعد از هر تغییر، کل آبجکت را با `save_settings` (debounce ~250ms) می‌فرستد.
Rust پس از ذخیره، عوارض جانبی را اعمال می‌کند: تم، آیکون tray، tooltip، منوی tray، autostart، ویجت.

## دستورها (invoke)

```ts
get_settings(): Settings
save_settings(settings: Settings): Settings     // جایگزینی کامل + اعمال عوارض
reset_settings(): Settings                       // برگشت به پیش‌فرض
set_autostart(enabled: boolean): boolean
get_autostart(): boolean
get_theme(): { mode: "light"|"dark", system: "light"|"dark" }  // mode طبق تنظیم، system تم ویندوز
get_today_info(): TodayInfo
get_month_events(year: number, month: number): MonthEvents
refresh_month_events(year: number, month: number): MonthEvents  // صرف‌نظر از کش، تلاش تازه (با احترام به backoff)
clear_events_cache(): void
copy_text(text: string): void
hide_popup(): void
show_popup(): void
quit_app(): void
```

`TodayInfo`:
```ts
{
  jalali: { year: number; month: number; day: number; weekday: number }, // weekday: 0=شنبه..6=جمعه
  gregorian: { year: number; month: number; day: number },
  lunar: { year: number; month: number; day: number } | null,  // از کش مناسبت‌ها
  holiday: boolean,
  events: string[],
  labels: { slash: string; long: string; iso: string }  // با ارقام طبق تنظیم
}
```

- `TodayInfo.lunar` هم با `lunarOffset` تنظیمات شیفت می‌خورد (از کش روزِ
  `today + offset` خوانده می‌شود).
- `TodayInfo.events` و `holiday` با همان قاعده‌ی مناسبت‌های قمری/تعطیلات ساخته
  می‌شوند: مناسبت‌های قمریِ روزِ `today + offset` + مناسبت‌های ثابتِ خود امروز
  (بخش «شیفت تاریخ قمری» را ببینید).

`MonthEvents`:
```ts
{
  year: number; month: number;
  source: "cache" | "network" | "none";
  stale: boolean;            // کش منقضی نمایش داده شد
  days: Record<string, {     // کلید = شماره‌ی روز
    holiday: boolean;
    events: Occasion[];      // به ترتیب خود API؛ ثابت و قمری در هم
    moon: { year: number; month: number; day: number } | null;
    gregorian: { year: number; month: number; day: number } | null;
  }>;
  error: string | null;
}
```

`Occasion`:
```ts
{
  text: string;              // رشته‌ی پاک‌شده (بدون بخش [..])
  lunar: { month: number; day: number } | null;  // فقط برای مناسبت قمری
}
```

- بخش `[ … ]` داخل رشته‌ی خام سرویس، تاریخ قمری مناسبت است (مثل `[ ٢١ رمضان ]`
  یا `[ ٣ جمادي الثانيه ]`)؛ Rust همان را در `Occasion.lunar` نگه می‌دارد و
  متن پاک‌شده را در `text`. اگر آن بخش تاریخ قمری نباشد (مثل `[ 8 March ]` یا
  `[ 1367 خورشیدی ]`)، یا کروشه‌ای نباشد، مناسبت «ثابت» است و `lunar: null`
  می‌گیرد. تشخیص تاریخ قمری پس از نرمال‌سازی ارقام عربی/فارسی، حروف
  عربی/فارسی (`ي/ی`، `ك/ک`، `ة/ه`)، اعراب، نیم‌فاصله و فاصله‌ها انجام می‌شود
  و نام ماه‌های دوازده‌گانه‌ی قمری (با املاهای رایج) تطبیق داده می‌شود.

## رویدادها (Rust → فرانت‌اند)

| event | payload | معنی |
|---|---|---|
| `settings-changed` | `Settings` | تنظیمات از سمت Rust عوض شد (مثلاً autostart یا موقعیت ویجت) |
| `theme-changed` | `{ mode, system }` | تم ویندوز عوض شد |
| `today-changed` | `TodayInfo` | نیمه‌شب/بیدارشدن/تغییر تاریخ |
| `events-updated` | `{ year, month }` | کش/شبکه ماه به‌روز شد؛ اگر همان ماه در حال نمایش است رفرش کن |
| `open-settings` | `{}` | از منوی tray: پنل تنظیمات را باز کن |
| `show-month` | `{ year, month }` | از منوی tray: پاپ‌آپ را به این ماه ببر |
| `copied` | `{ what: string }` | متن کپی شد (نمایش نشانگر کوتاه) |

فرانت‌اند هنگام `online` شدن پنجره، `refresh_month_events` ماه جاری را صدا بزند
(حداقل فاصله‌ی ۵ دقیقه را Rust خودش رعایت می‌کند).

## تنظیمات ظاهری هم‌شکل

- ارقام فارسی: تبدیل 0-9 به ۰-۹ در فرانت با تابع `toFaDigits` و در Rust با `fa_digits`.
  تنظیم ارقام **فقط** روی تاریخ شمسی و قمری اعمال می‌شود؛ تاریخ میلادی همیشه با
  ارقام لاتین (`13`) نمایش داده و کپی می‌شود (`formatGregorianIso` و
  `formatGregorianLong` بدون ارقام فارسی).
- چیدمان خانه‌ی روزها: عدد میلادی **بالای** عدد شمسی (`.day__greg`)، عدد شمسی
  وسط (`.day__num`) و عدد قمری **زیر** آن (`.day__lunar`). نمایش میلادی/قمری
  مستقل و اختیاری است.
- شیفت تاریخ قمری: `lunarOffset` در بازه‌ی −۳ تا +۳ (پیش‌فرض ۰) کل **لایه‌ی
  قمری** را جابه‌جا می‌کند: تاریخ قمری، مناسبت‌های قمری و تعطیلات قمری.
  داده‌ی API خام می‌ماند و ترکیب در نمایش انجام می‌شود (عدد مثبت = جلوتر).
  - تاریخ قمری هر روز: `EventsStore.lunarFor(date, offset)` یعنی تاریخ قمری
    روزِ `date + offset` (همان رفتار قبلی).
  - مناسبت‌ها: مناسبت ثابت (`lunar == null`) روی روز خودش می‌ماند؛ مناسبت
    قمری‌ای که API روی روز `S` گذاشته، در روز `S − offset` نمایش داده می‌شود
    (یعنی برای روز `D` مناسبت‌های قمریِ روزِ `D + offset` خوانده می‌شوند).
  - ترتیب: مناسبت‌های ثابت روزِ `D` (به ترتیب API) و بعد مناسبت‌های قمریِ
    `D + offset`؛ با `offset == 0` ترتیب خام API حفظ می‌شود.
  - تعطیلی: جمعه همیشه تعطیل است؛ رنگ تعطیلیِ مناسبت قمری با خودِ مناسبت به
    روز جدید منتقل می‌شود — روز قبلی آن رنگ را از دست می‌دهد، حتی اگر مناسبت
    ثابت (غیرتعطیل) هم داشته باشد. فقط تعطیلات رسمی شمسی ثابت می‌مانند.
    `holiday(D) = friday(D) || fixedHoliday(D) || lunarHoliday(D + offset)` که
    - `fixedHoliday(D)` = تاریخ‌های ثابت تعطیل رسمی شمسی (۱ تا ۴ فروردین، ۱۲ و
      ۱۳ فروردین، ۱۴ و ۱۵ خرداد، ۲۲ بهمن، ۲۹ اسفند) یا تعطیل بودن API برای
      روزی که هیچ مناسبت قمری ندارد؛
    - `lunarHoliday(S)` = تعطیل بودن API برای روزی که مناسبت قمری دارد.
    با `offset == 0` مجموعه‌ی این‌ها دقیقاً همان `holiday API || جمعه` قبلی است.
    فهرست تعطیلات شمسی در `src/events.ts` (`SOLAR_HOLIDAY_DATES`) و
    `today.rs` (`SOLAR_HOLIDAYS`) نگه داشته می‌شود و باید هم‌زمان به‌روز شود.
  - برای روزهای مرز ماه، ماه‌های مجاور (‎±۱) وقتی شیفت فعال است، از قبل
    prefetch می‌شوند.
  - توابع مشترک فرانت: `occasionsForDate(lookup, date, offset)`،
    `isHolidayDate(lookup, date, offset)` و
    `visibleEventsForDate(lookup, date, settings)` در `src/events.ts`.
    - سمت Rust: `today::displayed_occasions` و `today::compose_day` همین قاعده
      را برای `TodayInfo` و tooltip/tray پیاده می‌کنند.
- تم: `document.documentElement.dataset.theme = "light" | "dark"`؛ برای `auto` از `system` استفاده شود.
- رنگ accent در `--accent` (hex) و مشتقات `--accent-weak`.
- فونت: `font-family: Vazirmatn, "Segoe UI", system-ui` با `@font-face` محلی؛ حالت `system` فقط `"Segoe UI"`.

## API مناسبت‌ها (تأییدشده با درخواست واقعی)

- `GET https://pnldev.com/api/calender?year=Y&month=M` (بدون Auth؛ فقط year/month ارسال شود)
- پاسخ ماهانه: `{ "status": true, "result": { "1": { solar, moon, gregorian, holiday, event: string[] }, ... } }`
- پاسخ سالانه: `result[ماه][روز]` (ما فقط ماهانه می‌زنیم)
- `solar: {day,month,year,dayWeek}`, `moon: {day,month,year}`, `gregorian: {day,month,year}`, `holiday: boolean`, `event: string[]`
- رشته‌های event اغلب شامل `[ ١٥ رمضان ]` یا `[ 8 March ]` هستند؛ خودِ آن بخش
  از متن پاک می‌شود، ولی تاریخ قمریِ داخلش برای تفکیک و شیفت مناسبت قمری
  استخراج و در `Occasion.lunar` نگه داشته می‌شود.
- `day` تنها وقتی مفید است که تک‌روز بخواهیم؛ ما همیشه ماهانه می‌گیریم.
- `holiday=true` توسط سرور جدی گرفته نمی‌شود؛ هرگز ارسال نکن.

## کش مناسبت‌ها

- مسیر: `app_cache_dir()/events/<year>-<month>.json`
- شکل: `{ "version": 2, "fetchedAt": <unix seconds>, "year": Y, "month": M, "days": { ... } }`
- `version` نسخه‌ی ساختار کش است (`CACHE_VERSION` در `events_api.rs`). کشِ بدون
  نسخه یا با نسخه‌ی قدیمی نادیده گرفته می‌شود تا داده‌ی خام (با تاریخ قمری
  مناسبت‌ها) دوباره دریافت شود.
- ماه‌های گذشته: دائمی؛ ماه جاری/آینده: انقضای ۷ روز
- stale-while-revalidate: کش (حتی منقضی) فوراً برگردد و تازه‌سازی پس‌زمینه؛ بعد از موفقیت `events-updated` emit شود
- بدون کش و خطای شبکه: `source: "none"` + `error`؛ فوتر پاپ‌آپ پیام کم‌رنگ «برای نمایش مناسبت‌ها به اینترنت متصل شوید»
- backoff خطا: ۵ دقیقه، سپس دو برابر تا سقف ۳۰ دقیقه؛ هیچ حلقه‌ی بی‌وقفه‌ای نباشد
- prefetch: پس از موفقیت، ماه‌های ±۱ با هم‌زمانی ۱؛ در استارتاپ ماه جاری (اگر کش > ۲۴h) + ماه بعد
- `clear_events_cache` کل پوشه را پاک کند

## آیکون و Tray

- `icon.rs`: رندر RGBA با `ab_glyph` روی `image` خام (بدون وابستگی image)، اندازه ۳۲..۶۴ بر اساس DPI؛
  فونت embed: `Vazirmatn-FD-Bold.ttf` برای ارقام فارسی (ASCII→۰-۹) و `Vazirmatn-Bold.ttf` برای ارقام لاتین.
  رنگ عدد بر اساس تم ویندوز: تم روشن → عدد تیره (#1b1f27)، تم تیره → عدد روشن (#f2f5fa).
  عدد با **auto-fit** رندر می‌شود: مقیاس فونت طوری انتخاب می‌شود که جعبه‌ی جوهر متن
  (ink bbox) با حاشیه‌ی ~۱ پیکسل کل آیکون را پر کند (حالت `day`).
- حالت‌ها: `day` عدد روز؛ `static` آیکون ثابت برنامه. مقدار ناشناخته/قدیمی
  (مثل `dayMonth` حذف‌شده) به‌جای خراب کردن کل فایل تنظیمات، به `day` برمی‌گردد.
- به‌روزرسانی: نیمه‌شب، بیدارشدن از sleep، تغییر تاریخ/ساعت/منطقه (چک ۶۰ ثانیه‌ای)، تغییر تنظیمات.
- Tooltip: «پنجشنبه ۲۲ آبان ۱۴۰۵» + خط دوم مناسبت/تعطیل + خط میلادی در صورت روشن بودن.
- منو و tooltip منوی native ویندوز چپ‌به‌راست چیده می‌شود؛ به همین دلیل هر خطی که
  واژه و عدد را قاطی می‌کند با RLM (`\u{200F}`، تابع `today::rtl`) شروع می‌شود تا
  ترتیب راست‌به‌چپ درست رندر شود.

## منوی tray (فارسی، ارقام طبق تنظیم)

- امروز
- کپی تاریخ امروز ▸ (۱۴۰۵/۰۸/۲۲ | ۲۲ آبان ۱۴۰۵ | 2026-11-13)
- حالت ویجت دسکتاپ (checkbox)
- تنظیمات
- اجرا همراه ویندوز (checkbox)
- خروج

کلیک چپ = toggle پاپ‌آپ؛ راست = منو. پنجره با blur (اگر `closeOnBlur`) یا Esc پنهان شود.

## ویجت

- Rust: `always_on_bottom(true)`، شفاف، بدون دکوراسیون، skip_taskbar؛ اندازه small=200×92، full=300×330.
- موقعیت: از `widgetX/widgetY`؛ اگر null، گوشه‌ی پایین-چپ/راست مانیتور اصلی داخل work area.
  (RTL: پیش‌فرض پایین-چپ مانیتور)
- ذخیره موقعیت: در رویداد `Moved` پنجره با debounce ۴۰۰ms در store.
- درگ فقط با `data-tauri-drag-region` وقتی `widgetLocked=false` باشد.
- شفافیت: CSS `opacity` روی ریشه (`widgetOpacity/100`).
- گوشه‌ها: پنجره‌ی ویجت **دقیقاً** به اندازه‌ی کارت است، پس سایه‌ی بیرونی
  (`box-shadow` معمولی) در پنجره جا نمی‌شود و لبه‌ی مستطیلی پنجره آن را قطع
  می‌کند (اثر «گوشه‌ی تیز شفاف»). کارت ویجت نباید سایه‌ی بیرونی داشته باشد؛ فقط
  گوشه‌ی گرد (`border-radius: 12px`) و یک حلقه‌ی inset. همین قاعده برای کارت
  پاپ‌آپ هم برقرار است (پنجره ۳۴۴×۴۲۴ = کارت؛ `border-radius: var(--radius-lg)`
  + حلقه‌ی inset، بدون سایه‌ی بیرونی).
- راست‌کلیک: منوی سریع داخل ویجت (اندازه، قفل موقعیت، تنظیمات، پنهان‌کردن).

## مالکیت فایل‌ها (Write scopes)

- `rust-core`: کل `src-tauri/**` (به‌جز `src-tauri/icons/**` که Lead می‌سازد)
- `ui-frontend`: کل `src/**`، `index.html`، `widget.html` (به‌جز `src/assets/fonts/**`)
- `assets-docs`: `README.md`، `docs/*.md` (به‌جز همین فایل)، `LICENSE`
- `lead`: `package.json`، `tsconfig.json`، `vite.config.ts`، `tauri.conf.json` (هماهنگی)، `icons/**`، `scripts/**`

اگر به تغییر فایل خارج از scope نیاز بود، به Lead پیام بدهید؛ خودتان تغییر ندهید.
