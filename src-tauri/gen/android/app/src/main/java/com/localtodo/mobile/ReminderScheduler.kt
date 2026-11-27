package com.localtodo.mobile

import android.Manifest
import android.app.Activity
import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Looper
import android.os.PowerManager
import android.provider.Settings
import androidx.core.content.FileProvider
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.lang.ref.WeakReference
import java.util.Locale

/** 只依据 Rust 提供的权威计划注册本地闹钟，不读取或修改任务数据库。 */
object ReminderScheduler {
    private const val PREFS = "ltd_reminders"
    private const val KEY_STATE = "state"
    private const val REQUEST_NOTIFICATIONS = 4101
    private const val TEST_NOTIFICATION_ID = 0x4c544431
    private const val ACTION_FIRE = "com.localtodo.mobile.action.REMINDER_FIRE"
    private const val ACTION_REQUEST_NOTIFICATIONS =
        "com.localtodo.mobile.action.REQUEST_NOTIFICATIONS"
    private const val EXTRA_TASK_ID = "taskId"
    private const val EXTRA_TRIGGER_AT = "triggerAt"
    private const val EXTRA_REQUEST_CODE = "requestCode"
    const val CHANNEL_ID = "ltd_reminders"

    private val lock = Any()
    private var currentActivity = WeakReference<Activity>(null)

    private data class AlarmSpec(
        val taskId: String,
        val title: String,
        val body: String,
        val triggerAt: Long,
        val requestCode: Int,
    ) {
        val key: String get() = "$taskId\u0000$triggerAt"
    }

    private data class Fired(val taskId: String, val triggerAt: Long) {
        val key: String get() = "$taskId\u0000$triggerAt"
    }

    private data class State(
        val alarms: List<AlarmSpec> = emptyList(),
        val fired: List<Fired> = emptyList(),
        val shown: List<AlarmSpec> = emptyList(),
        val activeTaskIds: Set<String> = emptySet(),
    )

    @JvmStatic
    fun schedule(context: Context, itemsJson: String, activeTaskIdsJson: String) {
        synchronized(lock) {
            val appContext = context.applicationContext
            ensureChannel(appContext)
            val old = readState(appContext)
            val activeTaskIds = jsonStringArray(activeTaskIdsJson).toSet()
            val requested = parseSpecs(JSONArray(itemsJson))
            val now = System.currentTimeMillis()
            val usedCodes = old.shown.mapTo(mutableSetOf()) { it.requestCode }
            val desired = requested.mapNotNull { item ->
                if (item.triggerAt <= now) return@mapNotNull null
                val existing = old.alarms.firstOrNull { it.key == item.key }
                val code = existing?.requestCode
                    ?.takeIf { usedCodes.add(it) }
                    ?: nextRequestCode(item.taskId, usedCodes)
                item.copy(requestCode = code)
            }.distinctBy { it.key }
            val desiredKeys = desired.mapTo(mutableSetOf()) { it.key }

            old.alarms.filter { it.key !in desiredKeys }.forEach { cancelAlarm(appContext, it) }
            desired.forEach { registerAlarm(appContext, it) }

            val manager = notificationManager(appContext)
            val keptShown = old.shown.filter { shown ->
                val keep = shown.taskId in activeTaskIds
                if (!keep) manager.cancel(shown.requestCode)
                keep
            }
            writeState(
                appContext,
                State(desired, old.fired, keptShown.distinctBy { it.key }, activeTaskIds),
            )
        }
    }

    @JvmStatic
    fun cancelAll(context: Context) {
        synchronized(lock) {
            val appContext = context.applicationContext
            val state = readState(appContext)
            state.alarms.forEach { cancelAlarm(appContext, it) }
            val manager = notificationManager(appContext)
            state.shown.forEach { manager.cancel(it.requestCode) }
            // fired 必须保留到 Rust 数据库事务成功并调用 consumeFired。
            writeState(appContext, state.copy(alarms = emptyList(), shown = emptyList(), activeTaskIds = emptySet()))
        }
    }

    @JvmStatic
    fun statusJson(context: Context): String = synchronized(lock) {
        val appContext = context.applicationContext
        JSONObject()
            .put("exactAlarmAllowed", canScheduleExact(appContext))
            .put("notificationsAllowed", notificationsAllowed(appContext))
            .put("ignoringBatteryOptimizations", isIgnoringBatteryOptimizations(appContext))
            .put("scheduled", readState(appContext).alarms.size)
            .toString()
    }

    @JvmStatic
    fun firedJson(context: Context): String = synchronized(lock) {
        val array = JSONArray()
        readState(context.applicationContext).fired.forEach { item ->
            array.put(
                JSONObject()
                    .put("taskId", item.taskId)
                    .put("triggerAt", item.triggerAt),
            )
        }
        array.toString()
    }

    @JvmStatic
    fun consumeFired(context: Context, itemsJson: String) {
        synchronized(lock) {
            val appContext = context.applicationContext
            val consumed = parseFired(JSONArray(itemsJson)).mapTo(mutableSetOf()) { it.key }
            if (consumed.isEmpty()) return
            val state = readState(appContext)
            writeState(appContext, state.copy(fired = state.fired.filterNot { it.key in consumed }))
        }
    }

    /** true 仅表示权限已经允许；发起系统授权或跳转授权页时返回 false。 */
    @JvmStatic
    fun requestNotifications(context: Context): Boolean {
        val appContext = context.applicationContext
        ensureChannel(appContext)
        if (notificationsAllowed(appContext)) return true
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return false
        val activity = currentActivity.get()
        if (activity == null) {
            val launch = appContext.packageManager.getLaunchIntentForPackage(appContext.packageName)
                ?.setAction(ACTION_REQUEST_NOTIFICATIONS)
                ?.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            if (launch != null) runCatching { appContext.startActivity(launch) }
            return false
        }
        val request = {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) !=
                PackageManager.PERMISSION_GRANTED
            ) {
                activity.requestPermissions(
                    arrayOf(Manifest.permission.POST_NOTIFICATIONS),
                    REQUEST_NOTIFICATIONS,
                )
            }
        }
        if (Looper.myLooper() == Looper.getMainLooper()) request() else activity.runOnUiThread(request)
        return false
    }

    @JvmStatic
    fun showTestNotification(context: Context, title: String, body: String): Boolean {
        val appContext = context.applicationContext
        ensureChannel(appContext)
        if (!notificationsAllowed(appContext)) return false
        val notification = buildNotification(
            appContext,
            title.ifBlank { "Local To Do 测试提醒" },
            body,
            TEST_NOTIFICATION_ID,
            null,
            0L,
        )
        return runCatching {
            notificationManager(appContext).notify(TEST_NOTIFICATION_ID, notification)
            true
        }.getOrDefault(false)
    }

    @JvmStatic
    fun shareBackup(context: Context, path: String) {
        val appContext = context.applicationContext
        val file = File(path).canonicalFile
        require(file.isFile) { "备份文件不存在" }
        val extension = file.extension.lowercase(Locale.ROOT)
        val mime = when (extension) {
            "json" -> "application/json"
            "csv" -> "text/csv"
            else -> throw IllegalArgumentException("仅支持分享 JSON 或 CSV 备份")
        }
        val backupRoot = File(appContext.applicationInfo.dataDir, "backups").canonicalFile
        require(file.path.startsWith(backupRoot.path + File.separator)) {
            "备份文件不在应用备份目录内"
        }
        val shareDir = File(appContext.cacheDir, "shared-backups")
        check(shareDir.isDirectory || shareDir.mkdirs()) { "无法创建备份分享目录" }
        // Tauri 的 app_data_dir 是应用数据根目录；用私有缓存副本进行受控只读分享。
        val sharedFile = file.copyTo(File(shareDir, file.name), overwrite = true)
        val uri = FileProvider.getUriForFile(
            appContext,
            "${appContext.packageName}.fileprovider",
            sharedFile,
        )
        val share = Intent(Intent.ACTION_SEND)
            .setType(mime)
            .putExtra(Intent.EXTRA_STREAM, uri)
            .putExtra(Intent.EXTRA_SUBJECT, file.name)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        share.clipData = ClipData.newUri(appContext.contentResolver, file.name, uri)
        val chooser = Intent.createChooser(share, "分享 Local To Do 备份")
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        appContext.startActivity(chooser)
    }

    @JvmStatic
    fun openExactAlarmSettings(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return
        val intent = Intent(
            Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM,
            Uri.parse("package:${context.packageName}"),
        ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.applicationContext.startActivity(intent)
    }

    @JvmStatic
    fun openBatterySettings(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return
        val intent = Intent(
            Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
            Uri.parse("package:${context.packageName}"),
        ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.applicationContext.startActivity(intent)
    }

    @JvmStatic
    fun isIgnoringBatteryOptimizations(context: Context): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return true
        val power = context.getSystemService(Context.POWER_SERVICE) as PowerManager
        return power.isIgnoringBatteryOptimizations(context.packageName)
    }

    /** 系统广播只能恢复已存触发时刻；Core 的时区重算在应用下次启动时执行。 */
    @JvmStatic
    fun restore(context: Context) {
        val appContext = context.applicationContext
        val state = synchronized(lock) { readState(appContext) }
        val now = System.currentTimeMillis()
        val future = state.alarms.filter { it.triggerAt > now }
        val overdue = state.alarms.filter { it.triggerAt <= now }.sortedBy { it.triggerAt }
        schedule(appContext, specsJson(future), JSONArray(state.activeTaskIds.toList()).toString())
        overdue.forEach { deliver(appContext, it) }
    }

    /** 闹钟广播必须匹配持久化记录，失效或已被替换的 PendingIntent 不再展示。 */
    @JvmStatic
    fun dispatch(context: Context, taskId: String, triggerAt: Long, requestCode: Int) {
        synchronized(lock) {
            val state = readState(context.applicationContext)
            val spec = state.alarms.firstOrNull {
                it.taskId == taskId && it.triggerAt == triggerAt && it.requestCode == requestCode
            } ?: return
            deliverLocked(context.applicationContext, spec)
        }
    }

    fun onActivityResumed(activity: Activity) {
        currentActivity = WeakReference(activity)
        if (activity.intent?.action == ACTION_REQUEST_NOTIFICATIONS) {
            activity.intent.action = null
            requestNotifications(activity)
        }
    }

    fun onActivityPaused(activity: Activity) {
        if (currentActivity.get() === activity) currentActivity = WeakReference(null)
    }

    fun ensureChannel(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val manager = notificationManager(context)
        if (manager.getNotificationChannel(CHANNEL_ID) != null) return
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL_ID, "任务提醒", NotificationManager.IMPORTANCE_HIGH).apply {
                description = "来自本地任务的到期提醒"
            },
        )
    }

    fun canScheduleExact(context: Context): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return true
        return alarmManager(context).canScheduleExactAlarms()
    }

    private fun deliver(context: Context, spec: AlarmSpec) {
        synchronized(lock) { deliverLocked(context.applicationContext, spec) }
    }

    private fun deliverLocked(context: Context, spec: AlarmSpec) {
        val state = readState(context)
        if (state.shown.any { it.key == spec.key }) {
            writeState(context, state.copy(alarms = state.alarms.filterNot { it.key == spec.key }))
            return
        }
        val fired = Fired(spec.taskId, spec.triggerAt)
        val next = state.copy(
            alarms = state.alarms.filterNot { it.key == spec.key },
            fired = (state.fired.filterNot { it.key == fired.key } + fired),
            shown = state.shown.filterNot { it.key == spec.key } + spec,
        )
        // 先同步落盘，再展示通知，进程被杀也不会丢失待确认的 fired。
        writeState(context, next)
        ensureChannel(context)
        val notification = buildNotification(
            context,
            spec.title.ifBlank { "任务提醒" },
            spec.body,
            spec.requestCode,
            spec.taskId,
            spec.triggerAt,
        )
        runCatching { notificationManager(context).notify(spec.requestCode, notification) }
    }

    private fun buildNotification(
        context: Context,
        title: String,
        body: String,
        notificationId: Int,
        taskId: String?,
        triggerAt: Long,
    ): Notification {
        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)?.apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            if (taskId != null) putExtra(EXTRA_TASK_ID, taskId)
        }
        val contentIntent = launch?.let {
            PendingIntent.getActivity(
                context,
                notificationId,
                it,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        }
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(context, CHANNEL_ID)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(context).setPriority(Notification.PRIORITY_HIGH)
        }
        builder
            .setSmallIcon(context.applicationInfo.icon)
            .setContentTitle(title)
            .setContentText(body)
            .setStyle(Notification.BigTextStyle().bigText(body))
            .setCategory(Notification.CATEGORY_REMINDER)
            .setAutoCancel(true)
            .setWhen(if (triggerAt > 0) triggerAt else System.currentTimeMillis())
            .setShowWhen(true)
        if (contentIntent != null) builder.setContentIntent(contentIntent)
        return builder.build()
    }

    private fun registerAlarm(context: Context, spec: AlarmSpec) {
        val pending = requireNotNull(
            alarmPendingIntent(context, spec, PendingIntent.FLAG_UPDATE_CURRENT),
        )
        try {
            if (canScheduleExact(context)) {
                alarmManager(context).setExactAndAllowWhileIdle(
                    AlarmManager.RTC_WAKEUP,
                    spec.triggerAt,
                    pending,
                )
            } else {
                alarmManager(context).setAndAllowWhileIdle(
                    AlarmManager.RTC_WAKEUP,
                    spec.triggerAt,
                    pending,
                )
            }
        } catch (_: SecurityException) {
            // 用户可能在两次状态查询之间撤销精确闹钟权限，仍保留非精确提醒。
            alarmManager(context).setAndAllowWhileIdle(
                AlarmManager.RTC_WAKEUP,
                spec.triggerAt,
                pending,
            )
        }
    }

    private fun cancelAlarm(context: Context, spec: AlarmSpec) {
        val pending = alarmPendingIntent(context, spec, PendingIntent.FLAG_NO_CREATE) ?: return
        alarmManager(context).cancel(pending)
        pending.cancel()
    }

    private fun alarmPendingIntent(context: Context, spec: AlarmSpec, extraFlags: Int): PendingIntent? {
        val intent = Intent(context, ReminderReceiver::class.java)
            .setAction(ACTION_FIRE)
            .putExtra(EXTRA_TASK_ID, spec.taskId)
            .putExtra(EXTRA_TRIGGER_AT, spec.triggerAt)
            .putExtra(EXTRA_REQUEST_CODE, spec.requestCode)
        return if (extraFlags == PendingIntent.FLAG_NO_CREATE) {
            PendingIntent.getBroadcast(
                context,
                spec.requestCode,
                intent,
                PendingIntent.FLAG_NO_CREATE or PendingIntent.FLAG_IMMUTABLE,
            )
        } else {
            PendingIntent.getBroadcast(
                context,
                spec.requestCode,
                intent,
                extraFlags or PendingIntent.FLAG_IMMUTABLE,
            )
        }
    }

    private fun nextRequestCode(taskId: String, used: MutableSet<Int>): Int {
        var candidate = taskId.hashCode() and Int.MAX_VALUE
        while (!used.add(candidate)) candidate = if (candidate == Int.MAX_VALUE) 0 else candidate + 1
        return candidate
    }

    private fun parseSpecs(array: JSONArray): List<AlarmSpec> {
        val result = mutableListOf<AlarmSpec>()
        for (index in 0 until array.length()) {
            val item = array.optJSONObject(index) ?: continue
            val taskId = item.optString("taskId")
            val triggerAt = item.optLong("triggerAt", 0L)
            if (taskId.isBlank() || triggerAt <= 0L) continue
            result += AlarmSpec(
                taskId,
                item.optString("title"),
                item.optString("body"),
                triggerAt,
                item.optInt("requestCode", 0),
            )
        }
        return result
    }

    private fun parseFired(array: JSONArray): List<Fired> {
        val result = mutableListOf<Fired>()
        for (index in 0 until array.length()) {
            val item = array.optJSONObject(index) ?: continue
            val taskId = item.optString("taskId")
            val triggerAt = item.optLong("triggerAt", 0L)
            if (taskId.isNotBlank() && triggerAt > 0L) result += Fired(taskId, triggerAt)
        }
        return result
    }

    private fun specsJson(specs: List<AlarmSpec>): String {
        val array = JSONArray()
        specs.forEach { spec ->
            array.put(
                JSONObject()
                    .put("taskId", spec.taskId)
                    .put("title", spec.title)
                    .put("body", spec.body)
                    .put("triggerAt", spec.triggerAt)
                    .put("requestCode", spec.requestCode),
            )
        }
        return array.toString()
    }

    private fun jsonStringArray(text: String): List<String> {
        val array = JSONArray(text)
        return buildList {
            for (index in 0 until array.length()) {
                array.optString(index).takeIf { it.isNotBlank() }?.let(::add)
            }
        }
    }

    private fun readState(context: Context): State {
        val text = prefs(context).getString(KEY_STATE, null) ?: return State()
        val root = runCatching { JSONObject(text) }.getOrElse { return State() }
        val active = root.optJSONArray("activeTaskIds")?.let { array ->
            buildSet {
                for (index in 0 until array.length()) {
                    array.optString(index).takeIf { it.isNotBlank() }?.let(::add)
                }
            }
        } ?: emptySet()
        return State(
            alarms = root.optJSONArray("alarms")?.let(::parseSpecs).orEmpty(),
            fired = root.optJSONArray("fired")?.let(::parseFired).orEmpty(),
            shown = root.optJSONArray("shown")?.let(::parseSpecs).orEmpty(),
            activeTaskIds = active,
        )
    }

    private fun writeState(context: Context, state: State) {
        val active = JSONArray()
        state.activeTaskIds.sorted().forEach(active::put)
        val root = JSONObject()
            .put("version", 1)
            .put("alarms", JSONArray(specsJson(state.alarms)))
            .put("fired", JSONArray(firedArray(state.fired).toString()))
            .put("shown", JSONArray(specsJson(state.shown)))
            .put("activeTaskIds", active)
        check(prefs(context).edit().putString(KEY_STATE, root.toString()).commit()) {
            "无法保存提醒调度状态"
        }
    }

    private fun firedArray(items: List<Fired>): JSONArray {
        val array = JSONArray()
        items.forEach { item ->
            array.put(JSONObject().put("taskId", item.taskId).put("triggerAt", item.triggerAt))
        }
        return array
    }

    private fun notificationsAllowed(context: Context): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            return false
        }
        return notificationManager(context).areNotificationsEnabled()
    }

    private fun alarmManager(context: Context) =
        context.getSystemService(Context.ALARM_SERVICE) as AlarmManager

    private fun notificationManager(context: Context) =
        context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

    private fun prefs(context: Context) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
}
