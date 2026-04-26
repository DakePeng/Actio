// English strings. This file defines the canonical key set — every other
// locale must provide the same keys (enforced by the parity test in
// src/i18n/__tests__/parity.test.ts). Tokens like {name} are replaced by
// the second argument to t(key, vars).

export const en = {
  // Board / top toolbar
  'board.greeting.readyToAct': 'Ready to act?',
  'board.greeting.getThingsDone': "Let's get things done",
  'board.greeting.agenda': "What's on the agenda?",
  'board.greeting.makeMoves': 'Time to make moves',
  'board.greeting.boardAwaits': 'Your board awaits',
  'board.greeting.whatsNext': "What's next?",
  'board.greeting.pickUp': 'Pick up where you left off',
  'board.greeting.lockIn': "Let's lock in",
  'board.action.returnToTray': 'Return to tray',
  'board.action.captureNote': 'Capture note',

  // Recording tab
  'recording.startingUp': 'Starting up',
  'recording.listening': 'Listening…',

  // Live transcript
  'transcript.identifying': 'Identifying…',
  'transcript.unknown': 'Unknown',
  'transcript.translating': '·· translating',
  'transcript.translateError': 'Translation failed — retry',

  // Live tab — translation controls
  'live.translate.toggle': 'Translate',
  'live.translate.targetLabel': 'Target language',
  'live.translate.lang.en': 'English',
  'live.translate.lang.zh-CN': '简体中文',
  'live.translate.lang.ja': '日本語',
  'live.translate.lang.es': 'Español',
  'live.translate.lang.fr': 'Français',
  'live.translate.lang.de': 'Deutsch',

  // Settings tabs
  'settings.tab.general': 'General',
  'settings.tab.board': 'Board',
  'settings.tab.voice': 'Voice',
  'settings.tab.ai': 'AI',
  'settings.tab.shortcuts': 'Shortcuts',

  // Preferences section
  'settings.preferences.title': 'Preferences',
  'settings.preferences.theme': 'Theme',
  'settings.preferences.theme.light': 'Light',
  'settings.preferences.theme.system': 'System',
  'settings.preferences.theme.dark': 'Dark',
  'settings.preferences.notifications': 'Notifications',
  'settings.preferences.notifications.sub': 'Show alerts for new reminders',
  'settings.preferences.launchAtLogin': 'Launch at login',
  'settings.preferences.launchAtLogin.sub': 'Start Actio automatically when you log in',
  'settings.preferences.language': 'Language',
  'settings.preferences.language.sub': 'Applies to the app interface and enrollment passages',
  'settings.preferences.language.en': 'English',
  'settings.preferences.language.zh': '简体中文',

  // Audio settings
  'settings.audio.inputTitle': 'Audio Input',
  'settings.audio.microphone': 'Microphone',
  'settings.audio.noDevices': 'No devices found',
  'settings.audio.defaultSuffix': ' (default)',
  'settings.audio.speakerTitle': 'Speaker Recognition',
  'settings.audio.speakerHint':
    'How confident the app must be before labelling a transcript segment with a known speaker. Tentative matches show a ? badge; below the tentative threshold the segment is left unattributed. Continuity window lets a recent confirmed speaker inherit subsequent weak segments so one speech turn renders under one speaker. Changes take effect on the next pipeline restart.',
  'settings.audio.confirmThreshold': 'Confirm threshold',
  'settings.audio.tentativeThreshold': 'Tentative threshold',
  'settings.audio.minSpeechDuration': 'Min speech duration',
  'settings.audio.continuityWindow': 'Continuity window',
  'settings.audio.continuityOff': 'Off',
  'settings.audio.saveFailed': 'Failed to save',

  // Model setup
  'settings.models.title': 'Speech Models',
  'settings.models.downloadFrom': 'Download from',
  'settings.models.commonModels': 'Common Models',
  'settings.models.commonModels.sub':
    'Speaker-embedding model used for voiceprint enrollment and recognition. Switching models invalidates previous enrollments.',
  'settings.models.asrModels': 'ASR Models',
  'settings.models.preview': ' (preview)',
  'settings.models.downloaded': 'Downloaded',
  'settings.models.streaming': 'Streaming',
  'settings.models.offline': 'Offline',
  'settings.models.download': 'Download ({size} MB)',
  'settings.models.downloadOther': 'Another download in progress…',
  'settings.models.delete': 'Delete',
  'settings.models.deleteTitle': 'Delete {name} from disk',
  'settings.models.deleteConfirm':
    'Delete {name}? The files will be removed from disk. You can re-download later.',
  'settings.models.switchEmbeddingConfirm':
    'Switching embedding models will invalidate previously-enrolled voiceprints. Continue?',
  'settings.models.switchEmbeddingConfirmAction': 'Switch model',
  'settings.models.downloading': 'Downloading: {label} — {file}',
  'settings.models.preparing': 'Preparing...',
  'settings.models.cancel': 'Cancel',
  'settings.models.cancelTitle': 'Cancel download',
  'settings.models.lang.all': 'All',
  'settings.models.lang.chinese': 'Chinese',
  'settings.models.lang.english': 'English',
  'settings.models.lang.korean': 'Korean',
  'settings.models.lang.french': 'French',
  'settings.models.lang.multilingual': 'Multilingual',

  // Voiceprint enrollment
  'voiceprint.title': 'Record voiceprint for {name}',
  'voiceprint.readHint': 'Read this aloud at a normal volume:',
  'voiceprint.arming': 'Arming microphone…',
  'voiceprint.listening': 'Listening…',
  'voiceprint.waitingSound': 'Waiting for sound…',
  'voiceprint.aria.meter': 'Microphone input level',
  'voiceprint.aria.captured': '{captured} of {target} clips captured',
  'voiceprint.rejection.tooShort': 'That was too short — try reading the whole line.',
  'voiceprint.rejection.tooLong': 'That was too long — keep each take under 30 seconds.',
  'voiceprint.rejection.lowQuality':
    'Audio was too quiet or noisy — try speaking up a bit.',
  'voiceprint.success.title': '{name} is enrolled!',
  'voiceprint.success.sub': 'Their voice will now be recognised in transcripts.',
  'voiceprint.cancelled': 'Enrollment cancelled.',
  'voiceprint.cancel': 'Cancel',
  'voiceprint.passageSet.label': 'Passage set',
  'voiceprint.passageSet.en': 'English',
  'voiceprint.passageSet.zh': '中文',
  'voiceprint.passageSet.mixed': 'Mixed',

  // Feedback toasts
  'feedback.loadRemindersFailed': 'Unable to load reminders from the backend',
  'feedback.reminderAdded': 'Reminder added to the board',
  'feedback.saveReminderFailed': 'Unable to save reminder right now',
  'feedback.updateReminderFailed': 'Unable to update reminder right now',
  'feedback.labelCreated': 'Label created',
  'feedback.createLabelFailed': 'Unable to create label right now',
  'feedback.labelDeleted': 'Label deleted',
  'feedback.deleteLabelFailed': 'Unable to delete label right now',
  'feedback.updateLabelFailed': 'Unable to update label right now',
  'feedback.reminderArchived': 'Reminder archived',
  'feedback.archiveReminderFailed': 'Unable to archive reminder right now',
  'feedback.restoredToBoard': 'Restored to board',
  'feedback.restoreReminderFailed': 'Unable to restore reminder right now',
  'feedback.deletedPermanently': 'Deleted permanently',
  'feedback.deleteReminderFailed': 'Unable to delete reminder right now',
  'feedback.prioritySet': 'Priority set to {priority}',
  'feedback.updatePriorityFailed': 'Unable to update priority right now',
  'feedback.labelsUpdated': 'Labels updated',
  'feedback.updateLabelsFailed': 'Unable to update labels right now',
  'feedback.filtersCleared': 'Filters cleared',
  'feedback.noActionItems': 'No action items found in your note',
  'feedback.extractedSingle': 'Extracted 1 reminder',
  'feedback.extractedMany': 'Extracted {count} reminders',
  'feedback.extractFailed': "Couldn't extract reminders",
  'feedback.llmNotConfiguredFormMode': 'Language model is not configured, so quick capture opened in form mode',
  'feedback.listeningOn': 'Listening on',
  'feedback.listeningOff': 'Listening off',
  'feedback.listeningToggleFailed': "Couldn't change listening state",

  // Standby tray (collapsed + expanded)
  'tray.aria.drag': 'Drag to reposition',
  'tray.aria.openBoard': 'Open board',
  'tray.aria.listening': 'Listening',
  'tray.aria.transcribing': 'Transcribing',
  'tray.aria.toggleListening.on': 'Listening — click to mute',
  'tray.aria.toggleListening.off': 'Muted — click to start listening',
  'tray.tooltip.listening': 'Listening',
  'tray.tooltip.muted': 'Muted',
  'tray.status.transcribing': 'Transcribing...',
  'tray.status.listening': 'Listening...',
  'tray.viewFullBoard': 'View full board',
  'tray.swipe.done': 'Done',
  'tray.swipe.confirm': 'Confirm',

  // Top-level tab bar
  'tab.people': 'People',
  'tab.live': 'Live',
  'tab.board': 'Board',
  'tab.needsReview': 'Needs review',
  'tab.archive': 'Archive',
  'tab.settings': 'Settings',

  // Live tab
  'live.header.on': 'Listening',
  'live.header.off': 'Muted',
  'live.listeningSince': 'Listening since {time} • {duration}',
  'live.pausedHint': 'Listening is paused. Turn it on in the tray or here to start capturing.',

  // Needs-review queue (medium-confidence auto-extracted items)
  'needsReview.empty.title': 'Nothing to review',
  'needsReview.empty.body': "Uncertain items the assistant isn't sure about will show up here so you can confirm or dismiss them.",
  'needsReview.confirm': 'Confirm',
  'needsReview.dismiss': 'Dismiss',
  'needsReview.confirmAria': 'Confirm — move to Board',
  'needsReview.dismissAria': 'Dismiss — archive',
  'needsReview.sourceSpeaker': 'From {name}',
  'feedback.reminderConfirmed': 'Moved to Board',
  'feedback.reminderDismissed': 'Dismissed',

  // Trace inspector (provenance for auto-extracted cards)
  'card.trace.show': 'Show context',
  'card.trace.hide': 'Hide context',
  'card.trace.loading': 'Loading transcript…',
  'card.trace.empty': 'No transcript available for this card.',
  'card.trace.unknownSpeaker': 'Unknown',
  'card.trace.error': "Couldn't load transcript.",

  // Always-listening + windowed extractor settings
  'settings.audio.extractionTitle': 'Background action extraction',
  'settings.audio.extractionHint':
    'Actio scans your recent conversation on a rolling schedule and turns commitments into cards. High-confidence items land on the Board; uncertain ones go to Needs review.',
  'settings.audio.alwaysListening': 'Always listening',
  'settings.audio.alwaysListeningHint':
    'Keep the pipeline running whenever Actio is open so new action cards can appear without opening the Recording tab.',
  'settings.audio.windowLength': 'Window length',
  'settings.audio.windowStep': 'New window every',
  'settings.audio.extractionTick': 'Scheduler tick',
  'settings.audio.minutes': '{n} min',
  'settings.audio.seconds': '{n} s',
  'settings.audio.days': '{n} d',
  'settings.audio.batchTitle': 'Batch clip processing',
  'settings.audio.batchHint':
    'Clips of recorded audio are processed in the background to produce transcripts and group voices. Tune how clips are sized, how aggressively voices are merged, and how long raw audio is kept.',
  'settings.audio.useBatchPipeline': 'Use batch clip pipeline',
  'settings.audio.useBatchPipelineHint':
    'On by default. Splits transcription into a live path (dictation / translation) and a batch path that re-transcribes 5-min clips for the archive with global speaker clustering. Turn off only if you need the legacy single-pipeline behaviour. Restart the app after changing this.',
  'settings.audio.legacyOnlyHint':
    'The window-length / step / tick controls below only apply when the batch pipeline is off (see "Use batch clip pipeline" further down).',
  'settings.audio.liveAsrModel': 'Live ASR model',
  'settings.audio.liveAsrModelHint':
    'Used for dictation and live translation. Streaming models give the lowest latency; non-streaming (offline) models trade ~1-2 s of delay for higher accuracy and language coverage.',
  'settings.audio.liveAsrFallback': 'Fall back to the model picker below',
  'settings.audio.archiveAsrModel': 'Archive ASR model',
  'settings.audio.archiveAsrModelHint':
    'Used by the batch pipeline to re-transcribe 5-min clips for the archive. Only non-streaming models are listed — they have full audio context and produce better transcripts than the live path.',
  'settings.audio.archiveAsrFallback': 'Fall back to the model picker below',
  'settings.audio.streamingTag': 'streaming',
  'settings.audio.offlineTag': 'offline',
  'settings.audio.clipTarget': 'Clip target length',
  'settings.audio.clusterThreshold': 'Voice clustering threshold',
  'settings.audio.audioRetention': 'Audio retention',
  'settings.audio.provisionalGc': 'Drop unmatched voices after',

  // Board page filter bar
  'board.filter.priority': 'Priority',
  'board.filter.labels': 'Labels',
  'board.filter.clear': 'Clear filters',
  'board.priority.high': 'High',
  'board.priority.medium': 'Medium',
  'board.priority.low': 'Low',
  'board.filter.showingHigh': 'Showing high priority notes',
  'board.filter.showingMedium': 'Showing medium priority notes',
  'board.filter.showingLow': 'Showing low priority notes',
  'board.filter.priorityCleared': 'Priority filter cleared',
  'board.filter.labelApplied': '{name} filter applied',
  'board.filter.labelCleared': 'Label filter cleared',

  // Reminder cards
  'card.priority.high': 'High priority',
  'card.priority.medium': 'Medium priority',
  'card.priority.low': 'Low priority',
  'card.aiBadge': 'AI',
  'card.newBadge': 'New',
  'card.noDeadline': 'No deadline',
  'card.labelCount': '{count} labels',
  'card.editHint': 'Tap title or description to edit',
  'card.priorityLabel': 'Priority',
  'card.labelsLabel': 'Labels',
  'card.priorityName.high': 'High',
  'card.priorityName.medium': 'Medium',
  'card.priorityName.low': 'Low',
  'card.addLabel': '+ add',
  'card.aria.removeLabel': 'Remove {name}',
  'card.aria.editTitle': 'Edit title',
  'card.aria.editDescription': 'Edit description',
  'card.aria.editDueTime': 'Edit due time',
  'card.markDone': 'Mark done',
  'card.archivedToast': 'Archived: {title}',
  'card.titlePlaceholder': 'Reminder title',
  'card.descPlaceholder': 'Add a description…',
  'card.aria.extracting': 'Extracting reminder…',

  // Empty state
  'empty.noResults.title': 'No results found',
  'empty.noResults.copy': 'No reminders match your search or filters.',
  'empty.default.eyebrow': 'All caught up',
  'empty.default.title': 'The board is clear for now.',
  'empty.default.copy': 'Capture a new task to refill the board.',

  // Archive
  'archive.section.tasks': 'Tasks',
  'archive.section.clips': 'Clips',
  'archive.aria.sections': 'Archive sections',
  'archive.empty.title': 'Archive is empty',
  'archive.empty.desc': 'Deleted or archived notes will appear here.',
  'archive.empty.eyebrow': 'Clean Slate',
  'archive.selectAll': 'Select all',
  'archive.deselectAll': 'Deselect all',
  'archive.selectedCount': '{count} selected',
  'archive.action.restore': 'Restore',
  'archive.action.delete': 'Delete',
  'archive.action.star': 'Star',
  'archive.action.unstar': 'Unstar',
  'archive.clips.filter.all': 'All',
  'archive.clips.filter.starred': 'Starred',
  'archive.clips.aria.filter': 'Filter clips',
  'archive.clips.empty.starred': 'No starred clips yet. Star a clip to save it permanently.',
  'archive.clips.empty.all': 'No clips yet. Start recording to generate clips.',
  'archive.clip.showMore': 'Show more',
  'archive.clip.showLess': 'Show less',
  'archive.clip.today': 'Today {time}',
  'archive.clip.aria.star': 'Star clip',
  'archive.clip.aria.unstar': 'Unstar clip',
  'archive.clip.aria.delete': 'Delete clip',

  // People / speakers
  'people.addPerson': 'Add person',
  'people.namePlaceholder': 'Name',
  'people.save': 'Save',
  'people.saving': 'Saving…',
  'people.cancel': 'Cancel',
  'people.formHint':
    "After saving, you'll be asked to read three short passages so the app learns their voice.",
  'people.backendRequired': 'Backend required to manage speakers.',
  'people.retry': 'Retry',
  'people.loading': 'Loading…',
  'people.empty': 'No people added yet.',
  'people.confirmDelete': 'Delete {name}? Voiceprint is removed too.',
  'people.delete': 'Delete',
  'people.aria.swatch': 'Select color {color}',
  'people.aria.colorGroup': 'Color',
  'people.aria.record': 'Record voiceprint for {name}',
  'people.aria.edit': 'Edit {name}',
  'people.aria.delete': 'Delete {name}',
  'people.tooltip.record': 'Record voiceprint',
  'people.thisIsMe': 'This is me',

  // Candidate speakers (provisional rows from batch clip processing)
  'candidates.heading': 'Suggested people',
  'candidates.subtitle':
    'Voices the app heard but hasn’t enrolled yet. Promote to keep them, dismiss to drop.',
  'candidates.empty': 'No suggestions right now.',
  'candidates.lastHeard': 'Last heard {when}',
  'candidates.lastHeardUnknown': 'Never matched',
  'candidates.promote': 'Promote',
  'candidates.dismiss': 'Dismiss',
  'candidates.namePlaceholder': 'Their name',
  'candidates.save': 'Save',
  'candidates.cancel': 'Cancel',
  'candidates.confirmDismiss': 'Dismiss this suggestion?',
  'candidates.aria.promote': 'Promote {name}',
  'candidates.aria.dismiss': 'Dismiss {name}',

  // New reminder bar
  'newReminder.quickCapture': 'Quick capture',
  'newReminder.title.chat': 'Type or dictate a note',
  'newReminder.title.form': 'Add a note without leaving the board',
  'newReminder.copy.chat': 'Free-form note. Triage and labeling can happen after capture.',
  'newReminder.copy.form': 'Keep the entry short. Triage and labeling can happen after capture.',
  'newReminder.switchToForm': 'Switch to form',
  'newReminder.switchToChat': 'Switch to chat',
  'newReminder.tooltip.switchToForm': 'Switch to form view',
  'newReminder.tooltip.switchToChat': 'Switch to chat view',
  'newReminder.saveHint': 'Cmd/Ctrl + Enter to save',
  'newReminder.field.title': 'Title',
  'newReminder.field.details': 'Details',
  'newReminder.field.dueTime': 'Due time',
  'newReminder.placeholder.title': 'What needs attention?',
  'newReminder.placeholder.details': 'Optional context, owner, or timing',
  'newReminder.placeholder.dueTime': 'e.g. 2026-04-09T18:30:00Z',
  'newReminder.cancel': 'Cancel',
  'newReminder.addReminder': 'Add reminder',
  'newReminder.aria.close': 'Close',
  'newReminder.tooltip.close': 'Close (Esc)',

  // Settings – additional sections
  'settings.profile.title': 'About me',
  'settings.profile.name': 'Name',
  'settings.profile.namePlaceholder': 'e.g. Dake Peng',
  'settings.profile.aliases': 'Also called',
  'settings.profile.aliasesPlaceholder': 'Type and press Enter (e.g. DK, 彭大可)',
  'settings.profile.removeAlias': 'Remove alias',
  'settings.profile.bio': 'About you',
  'settings.profile.bioPlaceholder': 'A few sentences about who you are and what you care about. The action-item extractor reads this to decide what counts as relevant.',
  'settings.profile.save': 'Save',
  'settings.tray.title': 'Tray',
  'settings.tray.position': 'Tray position',
  'settings.tray.reset': 'Reset to default',
  'settings.recording.title': 'Recording',
  'settings.recording.autoClip': 'Auto-clip interval',
  'settings.recording.interval.min1': '1 minute',
  'settings.recording.interval.min2': '2 minutes',
  'settings.recording.interval.min5': '5 minutes',
  'settings.recording.interval.min10': '10 minutes',
  'settings.recording.interval.min30': '30 minutes',
  'settings.labels.title': 'Labels',
  'settings.labels.namePlaceholder': 'Label name…',
  'settings.labels.add': 'Add label',
  'settings.labels.pickColorFirst': 'Pick a color first',
  'settings.labels.aria.delete': 'Delete {name}',
  'settings.labels.aria.chooseColor': 'Choose color',
  'settings.labels.aria.pickColor': 'Pick color {color}',

  // Keyboard shortcuts
  'settings.shortcuts.title': 'Keyboard Shortcuts',
  'settings.shortcuts.group.global': 'Global shortcuts',
  'settings.shortcuts.group.tab': 'Tab navigation',
  'settings.shortcuts.group.card': 'Card navigation',
  'settings.shortcuts.globalSuffix': ' (global)',
  'settings.shortcuts.pressKeys': 'Press keys…',
  'settings.shortcuts.save': 'Save',
  'settings.shortcuts.cancel': 'Cancel',
  'settings.shortcuts.reset': 'Reset to defaults',
  'settings.shortcuts.saveFailed': 'Failed to save shortcut',
  'settings.shortcuts.resetFailed': 'Failed to reset shortcuts',
  'settings.shortcuts.action.toggle_board_tray': 'Toggle board / tray',
  'settings.shortcuts.action.toggle_listening': 'Toggle listening',
  'settings.shortcuts.action.start_dictation': 'Start dictation',
  'settings.shortcuts.action.new_todo': 'New to-do',
  'settings.shortcuts.action.tab_board': 'Board tab',
  'settings.shortcuts.action.tab_people': 'People tab',
  'settings.shortcuts.action.tab_live': 'Live tab',
  'settings.shortcuts.action.tab_needs_review': 'Needs-review tab',
  'settings.shortcuts.action.tab_archive': 'Archive tab',
  'settings.shortcuts.action.tab_settings': 'Settings tab',
  'settings.shortcuts.action.card_up': 'Card up',
  'settings.shortcuts.action.card_down': 'Card down',
  'settings.shortcuts.action.card_expand': 'Expand card',
  'settings.shortcuts.action.card_archive': 'Archive card',

  // ASR model descriptions (keyed by model id). English text mirrors the
  // backend catalog; the lookup falls back to the backend-supplied
  // description when a key is missing, so adding coverage for new models
  // is optional.
  'model.desc.zh_zipformer_14m':
    'Real-time streaming Chinese ASR. 14M params, int8. Very low latency.',
  'model.desc.zh_conformer':
    'Streaming Chinese ASR using Conformer architecture. Higher accuracy than Zipformer 14M but larger.',
  'model.desc.zh_lstm':
    'Streaming Chinese ASR using LSTM-transducer. Lighter than Conformer, good balance of size and accuracy.',
  'model.desc.en_zipformer_20m':
    'Real-time streaming English ASR. 20M params, int8. Very low latency.',
  'model.desc.en_zipformer':
    'Streaming English Zipformer transducer. Higher accuracy than the 20M variant.',
  'model.desc.en_zipformer_large':
    'Large streaming English Zipformer. Best streaming English accuracy but heavy.',
  'model.desc.en_zipformer_medium':
    'Medium-sized streaming English Zipformer. Good accuracy/size balance.',
  'model.desc.en_lstm': 'Streaming English LSTM-transducer. Compact and efficient.',
  'model.desc.ko_zipformer':
    'Real-time streaming Korean ASR. int8 Zipformer transducer.',
  'model.desc.fr_zipformer': 'Streaming French ASR. int8 Zipformer transducer.',
  'model.desc.zhen_zipformer_bilingual':
    'Streaming bilingual Chinese+English ASR in a single model. No language switching needed.',
  'model.desc.whisper_base':
    'OpenAI Whisper base, int8. Offline multilingual ASR with auto language detection. Processes utterances via VAD.',
  'model.desc.whisper_turbo':
    'OpenAI Whisper turbo, int8. Highest Whisper quality, fastest decoding. ~1 GB on disk. Processes utterances via VAD.',
  'model.desc.zipformer_ctc_zh_small':
    'Offline Chinese ASR using Zipformer CTC. Small int8 model, fast inference. Processes utterances via VAD.',
  'model.desc.paraformer_zh_small':
    'Offline bilingual Paraformer. Much smaller than FunASR Nano (~82 MB vs ~1 GB). Processes utterances via VAD.',
  'model.desc.moonshine_tiny_en':
    'Offline English ASR from Useful Sensors. ~27M params, int8. Processes utterances via VAD.',
  'model.desc.sense_voice_multi':
    'Offline multilingual ASR from FunAudioLLM. ~234M params, int8. Auto language detection. Processes utterances via VAD.',
  'model.desc.funasr_nano':
    'LLM-powered ASR with Qwen3-0.6B decoder. Highest quality but ~1 GB and slow on CPU. Not streaming.',
  'model.desc.campplus_zh_en':
    'Context-aware masking, bilingual. Small, fast, well-balanced. Recommended default.',
  'model.desc.campplus_zh':
    'Chinese-optimised CAM++. Slightly crisper on native zh speech than the bilingual variant.',
  'model.desc.campplus_en':
    "English-only CAM++ trained on VoxCeleb. Use for English-only scenarios where you want CAM++'s speed.",
  'model.desc.eres2net_base':
    'Multi-scale Res2Net blocks. More accurate than CAM++ on varied audio, slightly slower. 512-dim embeddings (4x per-voiceprint storage).',
  'model.desc.eres2netv2':
    'Newer ERes2Net architecture. Best accuracy in this set; largest file.',
  'model.desc.titanet_small_en':
    'NVIDIA NeMo, English-tuned. Different architecture than CAM++/ERes2Net; useful if you want a non-3D-Speaker alternative.',

  // Default label names seeded on first launch. Rendering code looks these
  // up when a label's stored name exactly matches one of the defaults — so
  // existing users see translated names without any DB migration, while
  // user-created labels stay untouched.
  'label.default.work': 'Work',
  'label.default.personal': 'Personal',
  'label.default.urgent': 'Urgent',
  'label.default.idea': 'Idea',
  'label.default.followUp': 'Follow-up',
  'label.default.meeting': 'Meeting',

  // AI / LLM settings
  'settings.llm.title': 'Language Models',
  'settings.llm.disabled': 'Disabled',
  'settings.llm.disabled.sub': ' — Action item extraction is off',
  'settings.llm.local': 'Local',
  'settings.llm.local.sub': ' — Run a model on this machine',
  'settings.llm.remote': 'Remote',
  'settings.llm.remote.sub': ' — Use an OpenAI-compatible API',
  'settings.llm.downloadFrom': 'Download from',
  'settings.llm.source.hf': 'Hugging Face',
  'settings.llm.source.hfMirror': 'HF Mirror (hf-mirror.com)',
  'settings.llm.source.modelScope': 'ModelScope (modelscope.cn)',
  'settings.llm.localModel': 'Local model',
  'settings.llm.loaded': 'Loaded',
  'settings.llm.spec': '~{size} MB download · ~{ram} MB RAM · {recRam} GB+ recommended',
  'settings.llm.cancel': 'Cancel',
  'settings.llm.downloading': 'Downloading model…',
  'settings.llm.loadingModel': 'Loading model…',
  'settings.llm.loadModel': 'Load model',
  'settings.llm.loadOnStartup': 'Load model at application startup',
  'settings.llm.loadOnStartupHint':
    'When enabled, the selected model downloads and loads automatically when Actio starts.',
  'settings.llm.endpoint': 'Endpoint',
  'settings.llm.port': 'Port:',
  'settings.llm.apply': 'Apply',
  'settings.llm.portError': 'Port must be 1024–65535',
  'settings.llm.applyPortFailed': 'Failed to apply port',
  'settings.llm.saveFailed': 'Failed to save',
  'settings.llm.remoteSaveFailed': 'Save failed',
  'settings.llm.testFailed': 'Test failed',
  'settings.llm.endpointUrlPrefix': 'Other tools can reach this at:',
  'settings.llm.endpointSharing':
    'Currently sharing the actio backend port. Pick a different port to expose the LLM separately.',
  'settings.llm.endpointSeparate':
    'LLM endpoint is on a separate port. The actio backend remains on port 3000.',
  'settings.llm.field.baseUrl': 'Base URL',
  'settings.llm.field.apiKey': 'API Key',
  'settings.llm.field.model': 'Model',
  'settings.llm.testing': 'Testing...',
  'settings.llm.testConnection': 'Test Connection',
} as const;

// Keys are the literal key set from `en`, but values are widened to
// `string` so other locales can assign Chinese glyphs without TypeScript
// complaining that they don't match the English literal type.
export type TKey = keyof typeof en;
export type Translations = Record<TKey, string>;
