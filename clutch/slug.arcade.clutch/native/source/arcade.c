#include "native/source/slug_arcade.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "native/vendor/raylib/src/raylib.h"

#define MAX_OVERLAYS 16
#define MAX_KEYS 8
#define MAX_EVENTS 65536
/* Without an input script, a headless run ends after this many frames. */
#define UNSCRIPTED_FRAMES 600

typedef struct {
    int32_t x, y, width, height;
    Color color;
} overlay;

/* One script line: from `frame` on, `keys` are held (headless mode). */
typedef struct {
    int64_t frame;
    int key_count;
    int keys[MAX_KEYS];
} input_event;

/* A `dump` or `shot` script line. */
typedef struct {
    int64_t frame;
    bool png;
} capture;

static struct {
    bool open;
    bool headless;
    int32_t width, height, rotate, scale;
    int32_t shown_width, shown_height;
    uint8_t *bits;
    size_t bits_length;
    Color *pixels;
    Texture2D texture;
    overlay overlays[MAX_OVERLAYS];
    int overlay_count;
    int64_t frames;
    /* Headless input and captures. */
    input_event events[MAX_EVENTS];
    int event_count;
    capture captures[MAX_EVENTS];
    int capture_count;
    int64_t end_frame;
    const char *out_dir;
    FILE *record;
    FILE *hashes;
    bool held[512];
    bool previous[512];
} arcade;

static const struct {
    const char *name;
    int key;
} KEYS[] = {
    {"left", KEY_LEFT},     {"right", KEY_RIGHT},   {"up", KEY_UP},
    {"down", KEY_DOWN},     {"space", KEY_SPACE},   {"enter", KEY_ENTER},
    {"escape", KEY_ESCAPE}, {"tab", KEY_TAB},       {"backspace", KEY_BACKSPACE},
    {"shift", KEY_LEFT_SHIFT}, {"control", KEY_LEFT_CONTROL},
    {"a", KEY_A}, {"b", KEY_B}, {"c", KEY_C}, {"d", KEY_D}, {"e", KEY_E},
    {"f", KEY_F}, {"g", KEY_G}, {"h", KEY_H}, {"i", KEY_I}, {"j", KEY_J},
    {"k", KEY_K}, {"l", KEY_L}, {"m", KEY_M}, {"n", KEY_N}, {"o", KEY_O},
    {"p", KEY_P}, {"q", KEY_Q}, {"r", KEY_R}, {"s", KEY_S}, {"t", KEY_T},
    {"u", KEY_U}, {"v", KEY_V}, {"w", KEY_W}, {"x", KEY_X}, {"y", KEY_Y},
    {"z", KEY_Z}, {"0", KEY_ZERO}, {"1", KEY_ONE}, {"2", KEY_TWO},
    {"3", KEY_THREE}, {"4", KEY_FOUR}, {"5", KEY_FIVE}, {"6", KEY_SIX},
    {"7", KEY_SEVEN}, {"8", KEY_EIGHT}, {"9", KEY_NINE},
};

static int key_code(const char *name, size_t length) {
    for (size_t i = 0; i < sizeof KEYS / sizeof KEYS[0]; i++) {
        if (strlen(KEYS[i].name) == length && strncmp(KEYS[i].name, name, length) == 0) {
            return KEYS[i].key;
        }
    }
    return -1;
}

/* Parses the headless script. Returns false on a malformed line. */
static bool read_script(const char *path) {
    arcade.end_frame = -1;
    if (path[0] == '\0' || strcmp(path, "-") == 0) {
        arcade.end_frame = UNSCRIPTED_FRAMES;
        return true;
    }
    FILE *file = fopen(path, "r");
    if (file == NULL) {
        return false;
    }
    char line[1024];
    bool ok = true;
    while (ok && fgets(line, sizeof line, file) != NULL) {
        char *hash = strchr(line, '#');
        if (hash != NULL) {
            *hash = '\0';
        }
        char *word = strtok(line, " \t\r\n");
        if (word == NULL) {
            continue;
        }
        if (strcmp(word, "end") == 0 || strcmp(word, "dump") == 0 || strcmp(word, "shot") == 0) {
            char *number = strtok(NULL, " \t\r\n");
            char *rest;
            if (number == NULL) {
                ok = false;
                break;
            }
            int64_t frame = strtoll(number, &rest, 10);
            if (*rest != '\0' || frame < 0) {
                ok = false;
            } else if (word[0] == 'e') {
                arcade.end_frame = frame;
            } else if (arcade.capture_count < MAX_EVENTS) {
                arcade.captures[arcade.capture_count++] = (capture){frame, word[0] == 's'};
            } else {
                ok = false;
            }
            continue;
        }
        char *rest;
        int64_t frame = strtoll(word, &rest, 10);
        if (*rest != '\0' || frame < 0 || arcade.event_count == MAX_EVENTS ||
            (arcade.event_count > 0 && frame < arcade.events[arcade.event_count - 1].frame)) {
            ok = false;
            break;
        }
        input_event *event = &arcade.events[arcade.event_count++];
        event->frame = frame;
        event->key_count = 0;
        for (char *key = strtok(NULL, " \t\r\n"); key != NULL; key = strtok(NULL, " \t\r\n")) {
            if (strcmp(key, "-") == 0) {
                continue;
            }
            int code = key_code(key, strlen(key));
            if (code < 0 || event->key_count == MAX_KEYS) {
                ok = false;
                break;
            }
            event->keys[event->key_count++] = code;
        }
    }
    fclose(file);
    return ok;
}

/* Sets the held keys for the current frame from the script. */
static void apply_script(void) {
    memcpy(arcade.previous, arcade.held, sizeof arcade.held);
    memset(arcade.held, 0, sizeof arcade.held);
    const input_event *current = NULL;
    for (int i = 0; i < arcade.event_count && arcade.events[i].frame <= arcade.frames; i++) {
        current = &arcade.events[i];
    }
    if (current != NULL) {
        for (int i = 0; i < current->key_count; i++) {
            arcade.held[current->keys[i]] = true;
        }
    }
}

int32_t slug_arcade_open(const char *title, int32_t width, int32_t height, int32_t rotate,
                         int32_t scale) {
    if (arcade.open) {
        return SLUG_ARCADE_ALREADY_OPEN;
    }
    if (width <= 0 || height <= 0 || width % 8 != 0 || width > 4096 || height > 4096 ||
        rotate < 0 || rotate > 1 || scale < 1 || scale > 16) {
        return SLUG_ARCADE_BAD_SIZE;
    }
    memset(&arcade, 0, sizeof arcade);
    arcade.width = width;
    arcade.height = height;
    arcade.rotate = rotate;
    arcade.scale = scale;
    arcade.shown_width = rotate ? height : width;
    arcade.shown_height = rotate ? width : height;
    arcade.bits_length = (size_t)width / 8 * (size_t)height;
    arcade.bits = calloc(arcade.bits_length, 1);
    arcade.pixels = calloc((size_t)arcade.shown_width * (size_t)arcade.shown_height, sizeof(Color));
    if (arcade.bits == NULL || arcade.pixels == NULL) {
        free(arcade.bits);
        free(arcade.pixels);
        return SLUG_ARCADE_BAD_SIZE;
    }

    SetTraceLogLevel(LOG_WARNING);
    const char *script = getenv("SLUG_ARCADE_HEADLESS");
    if (script != NULL) {
        arcade.headless = true;
        const char *out = getenv("SLUG_ARCADE_OUT");
        arcade.out_dir = out != NULL && out[0] != '\0' ? out : ".";
        const char *record = getenv("SLUG_ARCADE_RECORD");
        if (record != NULL && record[0] != '\0') {
            arcade.record = fopen(record, "wb");
        }
        const char *hashes = getenv("SLUG_ARCADE_HASHES");
        if (hashes != NULL && hashes[0] != '\0') {
            arcade.hashes = fopen(hashes, "wb");
        }
        if (!read_script(script)) {
            free(arcade.bits);
            free(arcade.pixels);
            return SLUG_ARCADE_BAD_SCRIPT;
        }
        apply_script();
    } else {
        InitWindow(arcade.shown_width * scale, arcade.shown_height * scale, title);
        if (!IsWindowReady()) {
            free(arcade.bits);
            free(arcade.pixels);
            return SLUG_ARCADE_NO_WINDOW;
        }
        SetTargetFPS(60);
        Image image = {arcade.pixels, arcade.shown_width, arcade.shown_height, 1,
                       PIXELFORMAT_UNCOMPRESSED_R8G8B8A8};
        arcade.texture = LoadTextureFromImage(image);
        SetTextureFilter(arcade.texture, TEXTURE_FILTER_POINT);
    }
    arcade.open = true;
    return SLUG_ARCADE_OK;
}

int32_t slug_arcade_overlay(int32_t x, int32_t y, int32_t width, int32_t height, uint32_t rgb) {
    if (!arcade.open) {
        return SLUG_ARCADE_NOT_OPEN;
    }
    if (arcade.overlay_count == MAX_OVERLAYS || width < 0 || height < 0) {
        return SLUG_ARCADE_BAD_SIZE;
    }
    arcade.overlays[arcade.overlay_count++] = (overlay){
        x, y, width, height,
        (Color){(unsigned char)(rgb >> 16), (unsigned char)(rgb >> 8), (unsigned char)rgb, 255}};
    return SLUG_ARCADE_OK;
}

int32_t slug_arcade_present(const uint8_t *bits, size_t length) {
    if (!arcade.open) {
        return SLUG_ARCADE_NOT_OPEN;
    }
    if (length != arcade.bits_length) {
        return SLUG_ARCADE_BAD_SIZE;
    }
    memcpy(arcade.bits, bits, length);
    return SLUG_ARCADE_OK;
}

static Color lit_color(int32_t x, int32_t y) {
    for (int i = arcade.overlay_count - 1; i >= 0; i--) {
        const overlay *o = &arcade.overlays[i];
        if (x >= o->x && x < o->x + o->width && y >= o->y && y < o->y + o->height) {
            return o->color;
        }
    }
    return WHITE;
}

/* Converts the presented bits to displayed pixels. */
static void render(void) {
    int32_t row_bytes = arcade.width / 8;
    for (int32_t v = 0; v < arcade.height; v++) {
        for (int32_t u = 0; u < arcade.width; u++) {
            bool lit = (arcade.bits[v * row_bytes + u / 8] >> (u % 8)) & 1;
            int32_t x = arcade.rotate ? v : u;
            int32_t y = arcade.rotate ? arcade.width - 1 - u : v;
            arcade.pixels[y * arcade.shown_width + x] = lit ? lit_color(x, y) : BLACK;
        }
    }
}

static int32_t write_png(const char *path, int32_t scale) {
    render();
    Image image = GenImageColor(arcade.shown_width * scale, arcade.shown_height * scale, BLACK);
    Color *out = image.data;
    for (int32_t y = 0; y < image.height; y++) {
        for (int32_t x = 0; x < image.width; x++) {
            out[y * image.width + x] = arcade.pixels[(y / scale) * arcade.shown_width + x / scale];
        }
    }
    bool ok = ExportImage(image, path);
    UnloadImage(image);
    return ok ? SLUG_ARCADE_OK : SLUG_ARCADE_WRITE_FAILED;
}

static void write_captures(void) {
    for (int i = 0; i < arcade.capture_count; i++) {
        if (arcade.captures[i].frame != arcade.frames) {
            continue;
        }
        char path[4096];
        snprintf(path, sizeof path, "%s/frame-%lld.%s", arcade.out_dir,
                 (long long)arcade.frames, arcade.captures[i].png ? "png" : "bin");
        if (arcade.captures[i].png) {
            write_png(path, 2);
        } else {
            FILE *file = fopen(path, "wb");
            if (file != NULL) {
                fwrite(arcade.bits, 1, arcade.bits_length, file);
                fclose(file);
            }
        }
    }
}

bool slug_arcade_frame(void) {
    if (!arcade.open) {
        return false;
    }
    if (arcade.headless) {
        write_captures();
        if (arcade.record != NULL) {
            fwrite(arcade.bits, 1, arcade.bits_length, arcade.record);
        }
        if (arcade.hashes != NULL) {
            /* FNV-1a, 64-bit, written little-endian. */
            uint64_t hash = 0xcbf29ce484222325u;
            for (size_t i = 0; i < arcade.bits_length; i++) {
                hash = (hash ^ arcade.bits[i]) * 0x100000001b3u;
            }
            unsigned char out[8];
            for (int i = 0; i < 8; i++) {
                out[i] = (unsigned char)(hash >> (8 * i));
            }
            fwrite(out, 1, sizeof out, arcade.hashes);
        }
        arcade.frames++;
        apply_script();
        return arcade.end_frame < 0 || arcade.frames < arcade.end_frame;
    }
    render();
    UpdateTexture(arcade.texture, arcade.pixels);
    BeginDrawing();
    ClearBackground(BLACK);
    DrawTexturePro(arcade.texture,
                   (Rectangle){0, 0, (float)arcade.shown_width, (float)arcade.shown_height},
                   (Rectangle){0, 0, (float)(arcade.shown_width * arcade.scale),
                               (float)(arcade.shown_height * arcade.scale)},
                   (Vector2){0, 0}, 0, WHITE);
    EndDrawing();
    arcade.frames++;
    return !WindowShouldClose();
}

static int named_key(const char *key) {
    return key_code(key, strlen(key));
}

bool slug_arcade_down(const char *key) {
    int code = named_key(key);
    if (!arcade.open || code < 0) {
        return false;
    }
    return arcade.headless ? arcade.held[code] : IsKeyDown(code);
}

bool slug_arcade_pressed(const char *key) {
    int code = named_key(key);
    if (!arcade.open || code < 0) {
        return false;
    }
    return arcade.headless ? arcade.held[code] && !arcade.previous[code] : IsKeyPressed(code);
}

int64_t slug_arcade_frame_count(void) {
    return arcade.frames;
}

int32_t slug_arcade_screenshot(const char *path, int32_t scale) {
    if (!arcade.open) {
        return SLUG_ARCADE_NOT_OPEN;
    }
    if (scale < 1 || scale > 16) {
        return SLUG_ARCADE_BAD_SIZE;
    }
    return write_png(path, scale);
}

void slug_arcade_close(void) {
    if (!arcade.open) {
        return;
    }
    if (!arcade.headless) {
        UnloadTexture(arcade.texture);
        CloseWindow();
    } else {
        if (arcade.record != NULL) {
            fclose(arcade.record);
        }
        if (arcade.hashes != NULL) {
            fclose(arcade.hashes);
        }
    }
    free(arcade.bits);
    free(arcade.pixels);
    arcade.open = false;
}
