package com.localtodo.desktop

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/** 开机后按最近一次持久化的提醒列表恢复系统闹钟，无需用户重新配对或同步 */
class ReminderBootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED) return
        runCatching { ReminderScheduler.restore(context) }
    }
}
