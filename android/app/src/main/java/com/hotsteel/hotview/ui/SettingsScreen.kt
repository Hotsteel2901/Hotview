package com.hotsteel.hotview.ui

import android.app.LocaleManager
import android.os.Build
import android.os.LocaleList
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.hotsteel.hotview.R

/**
 * Playback and interface preferences. Every option keeps the previous default
 * behaviour unless the user changes it.
 */
@Composable
fun SettingsScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    val settings = rememberSettingsStore()

    var softwareDecode by remember { mutableStateOf(settings.softwareDecode) }
    var autoPlay by remember { mutableStateOf(settings.autoPlayVideo) }
    var loop by remember { mutableStateOf(settings.loopVideos) }
    var gridColumns by remember { mutableStateOf(settings.gridColumns) }
    var keepScreenOn by remember { mutableStateOf(settings.keepScreenOn) }
    var fillScreen by remember { mutableStateOf(settings.fillScreen) }
    var startTab by remember { mutableStateOf(settings.startTab) }
    var backgroundPlayback by remember { mutableStateOf(settings.backgroundPlayback) }

    // Android 13+ needs the notification permission for the playback notice.
    val notificationPermission = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { }

    val localeManager = remember {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            context.getSystemService(LocaleManager::class.java)
        } else {
            null
        }
    }
    var languageTag by remember(localeManager) {
        mutableStateOf(localeManager?.applicationLocales?.toLanguageTags().orEmpty())
    }
    var languageDialog by remember { mutableStateOf(false) }

    val languageOptions = listOf(
        "" to stringResource(R.string.settings_language_system),
        "zh-CN" to "中文",
        "en" to "English",
        "ja" to "日本語",
        "de" to "Deutsch",
        "ru" to "Русский",
    )

    Box(
        Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .windowInsetsPadding(WindowInsets.statusBars)
                .padding(bottom = 48.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 4.dp, vertical = 4.dp),
            ) {
                IconButton(onClick = onBack) {
                    Icon(
                        imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                        contentDescription = stringResource(R.string.action_back),
                        tint = MaterialTheme.colorScheme.onSurface,
                    )
                }
                Text(
                    text = stringResource(R.string.settings_title),
                    style = MaterialTheme.typography.titleLarge,
                    fontWeight = FontWeight.SemiBold,
                )
            }

            SettingsCard {
                SectionTitle(stringResource(R.string.settings_decoder))
                Spacer(Modifier.height(10.dp))
                SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                    SegmentedButton(
                        selected = !softwareDecode,
                        onClick = {
                            softwareDecode = false
                            settings.softwareDecode = false
                        },
                        shape = SegmentedButtonDefaults.itemShape(index = 0, count = 2),
                    ) {
                        Text(stringResource(R.string.settings_decoder_hardware))
                    }
                    SegmentedButton(
                        selected = softwareDecode,
                        onClick = {
                            softwareDecode = true
                            settings.softwareDecode = true
                        },
                        shape = SegmentedButtonDefaults.itemShape(index = 1, count = 2),
                    ) {
                        Text(stringResource(R.string.settings_decoder_software))
                    }
                }
                Spacer(Modifier.height(8.dp))
                Text(
                    text = stringResource(R.string.settings_decoder_hint),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            SettingsCard {
                SwitchRow(
                    label = stringResource(R.string.settings_auto_play),
                    checked = autoPlay,
                ) {
                    autoPlay = it
                    settings.autoPlayVideo = it
                }
                SwitchRow(
                    label = stringResource(R.string.settings_loop),
                    checked = loop,
                ) {
                    loop = it
                    settings.loopVideos = it
                }
                SwitchRow(
                    label = stringResource(R.string.settings_keep_screen_on),
                    checked = keepScreenOn,
                ) {
                    keepScreenOn = it
                    settings.keepScreenOn = it
                }
                SwitchRow(
                    label = stringResource(R.string.settings_background_playback),
                    checked = backgroundPlayback,
                ) {
                    backgroundPlayback = it
                    settings.backgroundPlayback = it
                    if (it && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                        notificationPermission.launch(
                            android.Manifest.permission.POST_NOTIFICATIONS,
                        )
                    }
                }
            }

            SettingsCard {
                SectionTitle(stringResource(R.string.settings_grid_columns))
                Spacer(Modifier.height(8.dp))
                SegmentedOptions(
                    options = listOf("2" to "2", "3" to "3", "4" to "4"),
                    selected = gridColumns.toString(),
                ) { value ->
                    gridColumns = value.toInt()
                    settings.gridColumns = gridColumns
                }

                Spacer(Modifier.height(16.dp))
                SectionTitle(stringResource(R.string.settings_fit_mode))
                Spacer(Modifier.height(8.dp))
                SegmentedOptions(
                    options = listOf(
                        "fit" to stringResource(R.string.settings_fit_contain),
                        "fill" to stringResource(R.string.settings_fit_cover),
                    ),
                    selected = if (fillScreen) "fill" else "fit",
                ) { value ->
                    fillScreen = value == "fill"
                    settings.fillScreen = fillScreen
                }

                Spacer(Modifier.height(16.dp))
                SectionTitle(stringResource(R.string.settings_start_tab))
                Spacer(Modifier.height(8.dp))
                SegmentedOptions(
                    options = listOf(
                        "0" to stringResource(R.string.tabs_albums),
                        "1" to stringResource(R.string.tabs_photos),
                        "2" to stringResource(R.string.tabs_picked),
                    ),
                    selected = startTab.toString(),
                ) { value ->
                    startTab = value.toInt()
                    settings.startTab = startTab
                }
            }

            if (localeManager != null) {
                SettingsCard {
                    SectionTitle(stringResource(R.string.settings_language))
                    Spacer(Modifier.height(2.dp))
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable { languageDialog = true }
                            .padding(vertical = 10.dp),
                    ) {
                        Text(
                            text = languageOptions
                                .firstOrNull { it.first.equals(languageTag, ignoreCase = true) }
                                ?.second
                                ?: languageTag,
                            style = MaterialTheme.typography.bodyLarge,
                            modifier = Modifier.weight(1f),
                        )
                        Icon(
                            imageVector = Icons.Filled.ChevronRight,
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        }
    }

    if (languageDialog && localeManager != null) {
        AlertDialog(
            onDismissRequest = { languageDialog = false },
            title = { Text(stringResource(R.string.settings_language)) },
            text = {
                Column {
                    languageOptions.forEach { (tag, label) ->
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable {
                                    languageDialog = false
                                    languageTag = tag
                                    localeManager.applicationLocales =
                                        if (tag.isEmpty()) {
                                            LocaleList.getEmptyLocaleList()
                                        } else {
                                            LocaleList.forLanguageTags(tag)
                                        }
                                }
                                .padding(vertical = 2.dp),
                        ) {
                            RadioButton(
                                selected = languageTag.equals(tag, ignoreCase = true),
                                onClick = null,
                            )
                            Text(
                                text = label,
                                style = MaterialTheme.typography.bodyLarge,
                                modifier = Modifier.padding(start = 10.dp),
                            )
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { languageDialog = false }) {
                    Text(stringResource(R.string.action_close))
                }
            },
        )
    }
}

@Composable
private fun SettingsCard(content: @Composable ColumnScope.() -> Unit) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 6.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
        ),
    ) {
        Column(Modifier.padding(16.dp), content = content)
    }
}

@Composable
private fun SectionTitle(text: String) {
    Text(
        text = text,
        style = MaterialTheme.typography.titleSmall,
        color = MaterialTheme.colorScheme.primary,
    )
}

@Composable
private fun SwitchRow(label: String, checked: Boolean, onCheckedChange: (Boolean) -> Unit) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        modifier = Modifier
            .fillMaxWidth()
            .clickable { onCheckedChange(!checked) }
            .padding(vertical = 6.dp),
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.bodyLarge,
            modifier = Modifier.weight(1f),
        )
        Switch(checked = checked, onCheckedChange = onCheckedChange)
    }
}

/** A row of single-choice segmented buttons: `value to label` pairs. */
@Composable
private fun SegmentedOptions(
    options: List<Pair<String, String>>,
    selected: String,
    onSelect: (String) -> Unit,
) {
    SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
        options.forEachIndexed { index, (value, label) ->
            SegmentedButton(
                selected = selected == value,
                onClick = { onSelect(value) },
                shape = SegmentedButtonDefaults.itemShape(index = index, count = options.size),
            ) {
                Text(label, maxLines = 1)
            }
        }
    }
}
