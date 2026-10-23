#ifndef SLUG_ARCADE_H
#define SLUG_ARCADE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* slug_arcade_open, present, and screenshot status values. */
#define SLUG_ARCADE_OK 0
#define SLUG_ARCADE_ALREADY_OPEN 1
#define SLUG_ARCADE_BAD_SIZE 2
#define SLUG_ARCADE_BAD_SCRIPT 3
#define SLUG_ARCADE_NOT_OPEN 4
#define SLUG_ARCADE_WRITE_FAILED 5
#define SLUG_ARCADE_NO_WINDOW 6

/* Opens the display for 1-bit frames of `width` by `height` pixels. With
 * `rotate` 1 a frame is shown turned 90 degrees counter-clockwise, as on an
 * upright arcade monitor. When SLUG_ARCADE_HEADLESS is set no window opens and
 * input comes from the script it names. */
int32_t slug_arcade_open(const char *title, int32_t width, int32_t height, int32_t rotate,
                         int32_t scale);

/* Colours lit pixels inside a rectangle of the displayed (rotated) frame.
 * Later rectangles take precedence. Unlit pixels are black and lit pixels
 * outside every rectangle are white. */
int32_t slug_arcade_overlay(int32_t x, int32_t y, int32_t width, int32_t height, uint32_t rgb);

/* Copies the next frame: rows of `width` bits, least significant bit first. */
int32_t slug_arcade_present(const uint8_t *bits, size_t length);

/* Shows the presented frame, waits for the next 60 Hz tick, and reads input.
 * Returns false when the window closes or the headless script ends. */
bool slug_arcade_frame(void);

/* Whether a named key is held, or went down since the previous frame. */
bool slug_arcade_down(const char *key);
bool slug_arcade_pressed(const char *key);

/* The number of completed frames. */
int64_t slug_arcade_frame_count(void);

/* Writes the presented frame, as displayed and enlarged by `scale`, as PNG. */
int32_t slug_arcade_screenshot(const char *path, int32_t scale);

/* Closes the display. Closing twice is harmless. */
void slug_arcade_close(void);

#endif
