package com.localtodo.mobile

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/** 开机、应用更新及系统时间变化后恢复已存闹钟，不访问网络或同步服务。 */
class ReminderBootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action !in ACTIONS) return
        runCatching { ReminderScheduler.restore(context.applicationContext) }
    }

    companion object {
        private val ACTIONS = setOf(
            Intent.ACTION_BOOT_COMPLETED,
            Intent.ACTION_MY_PACKAGE_REPLACED,
            Intent.ACTION_TIME_CHANGED,
            Intent.ACTION_TIMEZONE_CHANGED,
        )
    }
}
