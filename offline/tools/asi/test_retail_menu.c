// Native-boundary self-check; build with MSVC x64 and user32.lib.
#include <assert.h>
#include "retail_menu.c"

static BYTE root[640], lobby_data[752], instance[512], item_data[128], field_data[24], result_data[16], context_data[160];
static INT64 root_ptr;
static BYTE mode = 1, found_argument = 1;
static int addresses, requests, recoveries;
static DWORD visibility_kind = 255;
static BYTE visibility_value;
static int visibility_writes;
static int skin_writes;
static UINT64 skin_value;
static UINT64 explicit_hero, explicit_skin, catalog_key, systems[80], owned[8];
static BYTE catalog_data[128], hero_data[64];
static int theme_reads;
static BYTE screen_visible;

static INT64 __fastcall record_of(INT64 map, const UINT64 *hero) {
    assert(map == (INT64)catalog_data + 0x58 && *hero == 0x02E000000000006EULL);
    return (INT64)hero_data;
}

static UINT64 __fastcall theme_of(UINT64 unlock) {
    theme_reads++;
    if (unlock == 0x02500000000004C9ULL) return 0x0A500000000016C9ULL;
    if (unlock == 0x02500000000004CAULL) return 0x0A500000000016CAULL;
    return 0;  // owned non-skin or unavailable unlock
}

static INT64 __fastcall asset_of(UINT64 theme) {
    return theme == 0x0A500000000016C9ULL ? 1 : 0;
}

static void __fastcall test_bool(INT64 out, BYTE value) {
    *(BYTE *)out = value;
    *(DWORD *)(out + 12) = 1;
}

static char __fastcall original(INT64 self, INT64 context, INT64 item, INT64 field, INT64 out) {
    test_bool(out, 0);
    if (*(USHORT *)(field + 16) == 29323) test_bool(out, screen_visible);
    if (*(USHORT *)(field + 16) == 1962) {
        *(DWORD *)(out + 12) = visibility_kind;
        *(BYTE *)out = visibility_value;
    }
    if (*(USHORT *)(field + 16) == 940) {
        *(UINT64 *)out = 0x02E000000000006EULL;
        *(DWORD *)(out + 12) = 4;
    }
    if (*(USHORT *)(field + 16) == 13241 || *(USHORT *)(field + 16) == 13242) {
        UINT64 input = *(USHORT *)(field + 16) == 13241 ? explicit_hero : (explicit_skin ? explicit_skin : skin_value);
        *(UINT64 *)out = input;
        *(DWORD *)(out + 12) = input ? 4 : 255;
    }
    return 1;
}

static INT64 __fastcall args(void) { return 123; }

static const char *__fastcall arg(INT64 args, const char *name, const char *fallback, BYTE *found) {
    assert(args == 123 && !strcmp(name, "lobbyServer"));
    *found = found_argument;
    return found_argument ? "127.0.0.1:3724" : fallback;
}

static INT64 __fastcall address(INT64 lobby, const char *text) {
    assert(lobby == (INT64)lobby_data && !strcmp(text, "127.0.0.1:3724"));
    addresses++;
    return 0;
}

static char value(void) {
    return dynamic_value(0, (INT64)context_data, (INT64)item_data, (INT64)field_data, (INT64)result_data);
}

static void __fastcall recover(INT64 store, const void *field, const void *value) {
    assert(store == 1234);
    if (*(const USHORT *)((const BYTE *)field + 16) == 13242) {
        assert(!((const BYTE *)field)[18] && ((const DWORD *)value)[3] == 4);
        skin_value = *(const UINT64 *)value;
        skin_writes++;
        return;
    }
    if (*(const USHORT *)((const BYTE *)field + 16) == 1962) {
        assert(!((const BYTE *)field)[18]);
        assert(*(const DWORD *)value == 1 && ((const DWORD *)value)[3] == 1);
        visibility_writes++;
        visibility_kind = 1;
        visibility_value = 1;
        return;
    }
    assert(*(const USHORT *)((const BYTE *)field + 16) == 1693 && !((const BYTE *)field)[18]);
    assert(!*(const DWORD *)value && ((const DWORD *)value)[3] == 2);
    recoveries++;
}

static INT64 __fastcall start(INT64 lobby, const char *name) {
    assert(lobby == (INT64)lobby_data && !strcmp(name, "Player"));
    requests++;
    value();  // synchronous re-entry must not request another connection
    assert(requests == 1);
    *(DWORD *)(lobby_data + 112) = 3;
    return 0;
}

int main(void) {
    assert(COMMAND_ARGS_RVA == 0x1866670 && ARGUMENT_RVA == 0x1839760);
    root_ptr = (INT64)root;
    app = &root_ptr;
    tournament = &mode;
    command_args = args;
    argument = arg;
    set_address = address;
    start_login = start;
    set_variable = recover;
    set_bool = test_bool;
    table_key = &catalog_key;
    hero_record = record_of;
    unlock_theme = theme_of;
    loaded_asset = asset_of;
    *(UINT64 *)(root + 320) = (UINT64)systems ^ (0x4E3494C633194824ULL - catalog_key);
    systems[70] = (UINT64)catalog_data;
    owned[0] = 0x0250000000000904ULL;  // an owned emote must never become a menu skin
    owned[2] = 0x02500000000004CAULL;  // unreadable theme must be skipped
    owned[4] = 0x02500000000004C9ULL;
    *(UINT64 **)hero_data = owned;
    *(UINT64 *)(hero_data + 8) = 3;
    *(UINT64 *)(hero_data + 0x30) = 0x02E000000000006EULL;
    overrides[0].evaluate = original;
    *(INT64 *)(context_data + 152) = 1234;
    *(INT64 *)(root + 632) = (INT64)lobby_data;
    *(INT64 *)(item_data + 120) = (INT64)instance;
    *(UINT64 *)(instance + 0x1E8) = 0x0580000000000194ULL;
    *(USHORT *)(field_data + 16) = 2275;
    field_data[18] = 1;
    *(DWORD *)(lobby_data + 112) = 1;

    *(UINT64 *)(instance + 0x1E8) = 0x0580000000000134ULL;
    *(USHORT *)(field_data + 16) = 8646;
    assert(value() && result_data[0] == 1);  // enable the authored visibility publisher
    field_data[18] = 0;
    assert(value() && result_data[0] == 0);
    field_data[18] = 1;
    mode = 0;
    assert(value() && result_data[0] == 0);
    mode = 1;
    *(USHORT *)(field_data + 16) = 29323;
    field_data[18] = 0;
    assert(value() && result_data[0] == 0);  // native screen visibility must remain untouched
    screen_visible = 1;
    assert(value() && result_data[0] == 1);
    screen_visible = 0;

    *(UINT64 *)(instance + 0x1E8) = 0x0580000000000B4AULL;
    *(USHORT *)(field_data + 16) = 940;
    field_data[18] = 0;
    assert(value() && *(UINT64 *)result_data == 0x02E000000000006EULL &&
           *(DWORD *)(result_data + 12) == 4 && visibility_writes == 0);
    assert(skin_writes == 1 && skin_value == 0x0A500000000016C9ULL);
    assert(value() && visibility_writes == 0);
    assert(skin_writes == 1 && theme_reads == 3);
    skin_value = 0;  // a new backdrop's graph keeps the same cached menu skin
    assert(value() && skin_writes == 2 && theme_reads == 3);
    skin_value = 0;
    explicit_hero = 0x02E000000000006EULL;
    assert(value() && skin_writes == 2);
    explicit_hero = 0;
    explicit_skin = 0x0A500000000016C7ULL;
    assert(value() && skin_writes == 2);
    explicit_skin = 0;
    menu_skin = 0;
    *(UINT64 *)(hero_data + 8) = 2;
    assert(value() && skin_writes == 2 && !menu_skin);  // native fallback with no loaded skin
    *(UINT64 *)(hero_data + 8) = 3;
    assert(value() && skin_writes == 3 && menu_skin == 0x0A500000000016C9ULL);
    assert(!memcmp(owned, (UINT64[]){0x0250000000000904ULL, 0, 0x02500000000004CAULL, 0,
                                  0x02500000000004C9ULL, 0, 0, 0}, sizeof owned));
    visibility_value = 0;  // an explicit native hide must survive
    assert(value() && !visibility_value && visibility_writes == 0);
    visibility_kind = 255;
    mode = 0;
    assert(value() && visibility_writes == 0);
    mode = 1;
    field_data[18] = 1;
    assert(value() && visibility_writes == 0);
    field_data[18] = 0;
    *(USHORT *)(field_data + 16) = 941;
    assert(value() && visibility_writes == 0);
    *(USHORT *)(field_data + 16) = 940;
    *(UINT64 *)(instance + 0x1E8) = 0x058000000000288BULL;
    assert(value() && visibility_writes == 0);
    *(UINT64 *)(instance + 0x1E8) = 0x0580000000000194ULL;
    *(USHORT *)(field_data + 16) = 2275;
    field_data[18] = 1;

    mode = 0;
    assert(value() && !result_data[0] && !requests);
    mode = 1;
    found_argument = 0;
    assert(value() && !result_data[0] && !requests);
    found_argument = 1;
    *(USHORT *)(field_data + 16) = 2276;
    assert(value() && !result_data[0] && !requests);
    *(USHORT *)(field_data + 16) = 2275;
    *(UINT64 *)(instance + 0x1E8) = 0x058000000000041AULL;
    assert(value() && !result_data[0] && !requests);
    *(UINT64 *)(instance + 0x1E8) = 0x0580000000000194ULL;

    assert(value() && result_data[0] == 1 && requests == 1 && addresses == 1);
    assert(value() && result_data[0] == 1 && requests == 1);
    *(DWORD *)(lobby_data + 112) = 7;
    assert(value() && result_data[0] == 1 && !recoveries);
    *(DWORD *)(lobby_data + 112) = 2;  // failed connection retains native recovery behavior
    assert(value() && !result_data[0] && recoveries == 1);
    assert(value() && !result_data[0] && requests == 1);
    puts("retail login, menu visibility and owned-skin checks passed");
    return 0;
}
