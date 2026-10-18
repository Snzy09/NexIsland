package com.coucou.android.core

/** The places the bottom bar leads to. */
enum class Tab { HOME, CHAT, NOTES, ALARM, SETTINGS }

/** Every screen of the app. Tabs are the roots; the rest are one level down. */
enum class Screen { HOME, CHAT, NOTES, ALARM, SETTINGS, HISTORY, GALLERY, WARDROBE, DIAGNOSTICS, LICENCES, SESSION, SCAN }

/**
 * Where the user is and where Back goes. Pure, so every rule is a unit test:
 * - the bar has Home, Chat, Notes, Alarm and Settings;
 * - the bar shows on the tab roots, and hides on every page below them and while the keyboard is open;
 * - Back goes up one level; on Home it is the system's (leave the app).
 */
object Nav {
    fun tabs(): List<Tab> = listOf(Tab.HOME, Tab.CHAT, Tab.NOTES, Tab.ALARM, Tab.SETTINGS)

    fun screenOf(tab: Tab): Screen = when (tab) {
        Tab.HOME -> Screen.HOME
        Tab.CHAT -> Screen.CHAT
        Tab.NOTES -> Screen.NOTES
        Tab.ALARM -> Screen.ALARM
        Tab.SETTINGS -> Screen.SETTINGS
    }

    /** The tab a screen belongs to, so the bar can highlight it; null for full-screen tasks. */
    fun tabOf(screen: Screen): Tab? = when (screen) {
        Screen.HOME -> Tab.HOME
        Screen.CHAT -> Tab.CHAT
        Screen.NOTES -> Tab.NOTES
        Screen.ALARM -> Tab.ALARM
        Screen.SETTINGS, Screen.HISTORY, Screen.GALLERY, Screen.WARDROBE, Screen.DIAGNOSTICS, Screen.LICENCES -> Tab.SETTINGS
        Screen.SESSION, Screen.SCAN -> null
    }

    fun barVisible(screen: Screen, keyboardOpen: Boolean): Boolean =
        !keyboardOpen && screen in setOf(Screen.HOME, Screen.CHAT, Screen.NOTES, Screen.ALARM, Screen.SETTINGS)

    /** One level up, or null when Back should leave the app. */
    fun back(screen: Screen): Screen? = when (screen) {
        Screen.HOME -> null
        Screen.CHAT, Screen.NOTES, Screen.ALARM, Screen.SETTINGS, Screen.SESSION, Screen.SCAN -> Screen.HOME
        Screen.HISTORY, Screen.GALLERY, Screen.WARDROBE, Screen.DIAGNOSTICS, Screen.LICENCES -> Screen.SETTINGS
    }
}
