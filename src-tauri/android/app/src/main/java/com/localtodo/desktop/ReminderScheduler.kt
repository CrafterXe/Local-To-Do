package com.localtodo.desktop

import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import org.json.JSONArray
import org.json.JSONObject

/**
 * 后台提醒的系统闹钟注册器。
 *
 * - 只负责注册/取消闹钟、持久化待触发列表、查询权限状态；
 * - 数据来源是 Rust 端根据同步镜像生成的计划（JSON），本类不读取任务数据；
 * - 待触发列表写入 SharedPreferences，供开机后恢复；
 * - 精确闹钟权限不可用时降级为非精确闹钟（仍会在后台触发，只是时间可能偏移）。
 */
object ReminderScheduler {
    private const val PREFS = "ltd_reminders"
    private const val KEY_ALARMS = "alarms"
    const val CHANNEL_ID = "ltd_reminders"
    const val EXTRA_TASK_ID = "taskId"
    const val EXTRA_TITLE = "title"
    const val EXTRA_BODY = "body"
    const val EXTRA_NOTIFICATION_ID = "notificationId"

    @JvmStatic
    fun schedule(context: Context, json: String) {
        ensureChannel(context)
        cancelAll(context)
        val items = runCatching { JSONArray(json) }.getOrElse { JSONArray() }
        val manager = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
        val exact = canScheduleExact(context)
        val stored = JSONArray()
        val now = System.currentTimeMillis()
        for (index in 0 until items.length()) {
            val item = items.optJSONObject(index) ?: continue
            val taskId = item.optString("taskId")
            val triggerAt = item.optLong("triggerAt", 0L)
            if (taskId.isEmpty() || triggerAt <= now) continue
            val title = item.optString("title")
            val body = item.optString("body")
            val notificationId = notificationId(taskId)
            val intent = Intent(context, ReminderReceiver::class.java).apply {
                putExtra(EXTRA_TASK_ID, taskId)
                putExtra(EXTRA_TITLE, title)
                putExtra(EXTRA_BODY, body)
                putExtra(EXTRA_NOTIFICATION_ID, notificationId)
            }
            val pending = PendingIntent.getBroadcast(
                context,
                notificationId,
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
            )
            if (exact) {
                manager.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, triggerAt, pending)
            } else {
                manager.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, triggerAt, pending)
            }
            stored.put(JSONObject().apply {
                put("taskId", taskId)
                put("title", title)
                put("body", body)
                put("triggerAt", triggerAt)
            })
        }
        prefs(context).edit().putString(KEY_ALARMS, stored.toString()).apply()
    }

    @JvmStatic
    fun cancelAll(context: Context) {
        val manager = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
        val stored = readStored(context)
        for (index in 0 until stored.length()) {
            val item = stored.optJSONObject(index) ?: continue
            val taskId = item.optString("taskId")
            if (taskId.isEmpty()) continue
            val intent = Intent(context, ReminderReceiver::class.java)
            val pending = PendingIntent.getBroadcast(
                context,
                notificationId(taskId),
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
            )
            manager.cancel(pending)
        }
        prefs(context).edit().remove(KEY_ALARMS).apply()
    }

    @JvmStatic
    fun statusJson(context: Context): String {
        val now = System.currentTimeMillis()
        var scheduled = 0
        val stored = readStored(context)
        for (index in 0 until stored.length()) {
            val item = stored.optJSONObject(index) ?: continue
            if (item.optLong("triggerAt", 0L) > now) scheduled += 1
        }
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        return JSONObject().apply {
            put("exactAlarmAllowed", canScheduleExact(context))
            put("notificationsAllowed", manager.areNotificationsEnabled())
            put("ignoringBatteryOptimizations", isIgnoringBatteryOptimizations(context))
            put("scheduled", scheduled)
        }.toString()
    }

    /** 是否已允许忽略电池优化（部分厂商会在熄屏后清理未加入白名单的应用） */
    @JvmStatic
    fun isIgnoringBatteryOptimizations(context: Context): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return true
        val power = context.getSystemService(Context.POWER_SERVICE) as PowerManager
        return power.isIgnoringBatteryOptimizations(context.packageName)
    }

    /** 打开系统的忽略电池优化授权页 */
    @JvmStatic
    fun openBatterySettings(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return
        val intent = Intent(
            Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
            Uri.parse("package:" + context.packageName)
        )
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        runCatching { context.startActivity(intent) }
    }

    @JvmStatic
    fun openExactAlarmSettings(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return
        val intent = Intent(
            Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM,
            Uri.parse("package:" + context.packageName)
        )
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        runCatching { context.startActivity(intent) }
    }

    /** 开机或应用更新后，按持久化的待触发列表重新注册 */
    fun restore(context: Context) {
        val stored = prefs(context).getString(KEY_ALARMS, null) ?: return
        schedule(context, stored)
    }

    /** 提醒已展示：从待触发列表中移除，防止重启后重复触发 */
    fun removeStored(context: Context, taskId: String) {
        val stored = readStored(context)
        val kept = JSONArray()
        for (index in 0 until stored.length()) {
            val item = stored.optJSONObject(index) ?: continue
            if (item.optString("taskId") != taskId) kept.put(item)
        }
        prefs(context).edit().putString(KEY_ALARMS, kept.toString()).apply()
    }

    fun ensureChannel(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (manager.getNotificationChannel(CHANNEL_ID) != null) return
        val channel = NotificationChannel(
            CHANNEL_ID,
            "任务提醒",
            NotificationManager.IMPORTANCE_HIGH
        ).apply {
            description = "来自本地任务的到期提醒"
        }
        manager.createNotificationChannel(channel)
    }

    fun canScheduleExact(context: Context): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return true
        val manager = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
        return manager.canScheduleExactAlarms()
    }

    private fun readStored(context: Context): JSONArray {
        val text = prefs(context).getString(KEY_ALARMS, null) ?: return JSONArray()
        return runCatching { JSONArray(text) }.getOrElse { JSONArray() }
    }

    private fun prefs(context: Context) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    private fun notificationId(taskId: String): Int = taskId.hashCode() and 0x7fffffff
}
