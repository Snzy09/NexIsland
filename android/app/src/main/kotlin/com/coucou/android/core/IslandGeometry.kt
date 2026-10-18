package com.coucou.android.core

import kotlin.math.max
import kotlin.math.min

/** A rectangle in pixels (the display cutout's bounding box at the top of the screen). */
data class PxRect(val left: Int, val top: Int, val right: Int, val bottom: Int) {
    val centerX: Int get() = (left + right) / 2
    val width: Int get() = right - left
}

/** Where the island sits and how big it is, in pixels. */
data class IslandBox(
    /** Horizontal offset of the island's centre from the screen's centre (negative = left). */
    val centerOffsetX: Int,
    /** Space at the top that the camera cut-out or status bar takes: content starts below it. */
    val topInset: Int,
    /** Width of the island when it is hidden in the notch. */
    val notchWidth: Int,
    val workingW: Int, val workingH: Int,
    val cardW: Int, val cardH: Int,
    val approvalW: Int, val approvalH: Int,
    val cornerSmall: Int, val cornerLarge: Int,
    /** Vertical offset from top edge of screen, placing the dynamic island symmetrically on the front camera. */
    val topOffset: Int = 0,
    /** Minimized pill width and height. */
    val pillW: Int = workingW,
    val pillH: Int = workingH,
)

/**
 * The island is centred on the camera cut-out (a centred punch-hole on Vivo Y28 / waterdrop on A12s),
 * its black merges with the hole, and its content starts under it.
 * Sized and positioned following Vivo Y28 format:
 * - Screen dimensions: 165.7 mm height, 76.0 mm width
 * - Front camera position: 0.5 cm (5.0 mm) from top of screen to camera aperture.
 * - Camera lens diameter: ~4.0 mm, center at 7.0 mm from top edge.
 */
object IslandGeometry {
    const val MARGIN_DP = 12
    /** The concave flare on each side where the island meets the top edge of the screen (the PC's `#island::before/after`). */
    const val EAR_DP = 14
    const val MAX_W_DP = 316
    const val WORKING_W_DP = 224 // narrow: it should look like it grows out of the camera hole
    const val MIN_NOTCH_DP = 44
    const val HOLE_PAD_DP = 10
    const val PILL_W_DP = 176
    const val PILL_H_DP = 36

    // Vivo Y28 physical specifications
    const val VIVO_Y28_HEIGHT_MM = 165.7f
    const val VIVO_Y28_WIDTH_MM = 76.0f
    const val VIVO_Y28_CAMERA_TOP_MM = 5.0f // 0.5 cm from top
    const val VIVO_Y28_CAMERA_CENTER_MM = 7.0f // 5.0 mm + 2.0 mm

    fun place(screenW: Int, density: Float, cutoutTop: PxRect?, statusBarH: Int): IslandBox {
        val estimatedScreenH = (screenW * (VIVO_Y28_HEIGHT_MM / VIVO_Y28_WIDTH_MM)).toInt()
        return place(screenW, estimatedScreenH, density, cutoutTop, statusBarH)
    }

    fun place(screenW: Int, screenH: Int, density: Float, cutoutTop: PxRect?, statusBarH: Int): IslandBox {
        fun dp(v: Int) = (v * density).toInt()
        // The flares sit outside the island's width, so they come off the room it may use.
        val maxW = min(screenW - 2 * dp(MARGIN_DP) - 2 * dp(EAR_DP), dp(MAX_W_DP))
        val cut = cutoutTop?.takeIf { it.width > 0 && it.bottom > 0 }
        val top = max(cut?.bottom ?: statusBarH, 0)
        // Keep the island on screen even when the cut-out is off-centre.
        val rawOffset = cut?.let { it.centerX - screenW / 2 } ?: 0
        val room = max(screenW / 2 - maxW / 2, 0)
        val offset = rawOffset.coerceIn(-room, room)
        // Hidden, the island is just a little wider than the camera hole, so it appears to grow out of it.
        val notch = min(max((cut?.width ?: 0) + dp(HOLE_PAD_DP), dp(MIN_NOTCH_DP)), maxW)

        // Vivo Y28 front camera center in pixels
        val vivoCameraCenterPx = (screenH.toFloat() * (VIVO_Y28_CAMERA_CENTER_MM / VIVO_Y28_HEIGHT_MM)).toInt()
        val cameraCenterY = cut?.let { (it.top + it.bottom) / 2 } ?: vivoCameraCenterPx
        val pillH = dp(PILL_H_DP)
        // Symmetrically center the pill on the camera hole vertically
        val topOffset = max((cameraCenterY - pillH / 2), dp(4))

        return IslandBox(
            centerOffsetX = offset, topInset = top, notchWidth = notch,
            workingW = min(dp(WORKING_W_DP), maxW), workingH = top + dp(38),
            cardW = maxW, cardH = top + dp(66),
            approvalW = maxW, approvalH = top + dp(132),
            cornerSmall = dp(14), cornerLarge = dp(22),
            topOffset = topOffset,
            pillW = min(dp(PILL_W_DP), maxW),
            pillH = pillH,
        )
    }

    /** Size for a kind of content. */
    fun sizeFor(box: IslandBox, kind: IslandSpec.Kind): Pair<Int, Int> = when (kind) {
        IslandSpec.Kind.WORKING -> box.workingW to box.workingH
        IslandSpec.Kind.APPROVAL -> box.approvalW to box.approvalH
        else -> box.cardW to box.cardH
    }
}
