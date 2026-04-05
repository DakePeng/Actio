"""Model loading and lifecycle management for FunASR and CAM++."""
import logging
import threading
from dataclasses import dataclass
from typing import Optional

logger = logging.getLogger("actio-worker.models")


@dataclass
class ModelState:
    vad_ready: bool = False
    asr_ready: bool = False
    speaker_ready: bool = False
    error_detail: str = ""


class ModelLoader:
    """Manages loading and caching of inference models.

    Models are loaded lazily on first request and cached for the lifetime of the process.
    """

    def __init__(self):
        self._lock = threading.Lock()
        self._state = ModelState()
        self._asr_model = None
        self._speaker_model = None
        self._vad_model = None

    @property
    def state(self) -> ModelState:
        return self._state

    def load_asr_model(self):
        """Load FunASR Paraformer-Streaming model for streaming ASR."""
        with self._lock:
            if self._asr_model is not None:
                return self._asr_model
            try:
                from funasr import AutoModel
                logger.info("Loading FunASR Paraformer-Streaming model...")
                self._asr_model = AutoModel(
                    model="paraformer-zh-streaming",
                    model_revision="v2.0.4",
                    disable_update=True,
                )
                self._state.asr_ready = True
                logger.info("FunASR model loaded successfully")
            except Exception as e:
                self._state.error_detail = f"ASR model load failed: {e}"
                logger.error(f"Failed to load ASR model: {e}")
                raise

    def load_speaker_model(self):
        """Load CAM++ speaker embedding model from 3D-Speaker."""
        with self._lock:
            if self._speaker_model is not None:
                return self._speaker_model
            try:
                from modelscope.pipelines import pipeline
                logger.info("Loading CAM++ speaker embedding model...")
                self._speaker_model = pipeline(
                    task="speaker-verification",
                    model="iic/speech_campplus_sv_zh-cn_16k-common",
                )
                self._state.speaker_ready = True
                logger.info("CAM++ model loaded successfully")
            except Exception as e:
                self._state.error_detail = f"Speaker model load failed: {e}"
                logger.error(f"Failed to load speaker model: {e}")
                raise

    def get_asr_model(self):
        if self._asr_model is None:
            self.load_asr_model()
        return self._asr_model

    def get_speaker_model(self):
        if self._speaker_model is None:
            self.load_speaker_model()
        return self._speaker_model
