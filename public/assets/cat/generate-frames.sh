#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

fps="${FPS:-10}"

if ! command -v ffmpeg >/dev/null 2>&1; then
  echo "ffmpeg is required. Install it first, then rerun this script." >&2
  exit 1
fi

convert_action() {
  local name="$1"
  local source=""
  local output_dir="${name}_frames"

  if [[ -f "${name}.gif" ]]; then
    source="${name}.gif"
  elif [[ -f "${name}.mov" ]]; then
    source="${name}.mov"
  elif [[ -f "${name}.webm" ]]; then
    source="${name}.webm"
  elif [[ -f "${name}.mp4" ]]; then
    source="${name}.mp4"
  else
    echo "skip ${name}: no ${name}.gif/.mov/.webm/.mp4 found"
    return 0
  fi

  mkdir -p "${output_dir}"
  find "${output_dir}" -maxdepth 1 -name 'frame_*.png' -delete

  echo "converting ${source} -> ${output_dir}/frame_%03d.png at ${fps}fps"
  ffmpeg -y -i "${source}" -vf "fps=${fps}" -start_number 1 "${output_dir}/frame_%03d.png"

  local count
  count="$(find "${output_dir}" -maxdepth 1 -name 'frame_*.png' | wc -l | tr -d ' ')"
  echo "generated ${count} frames for ${name}"
}

if [[ "$#" -gt 0 ]]; then
  for name in "$@"; do
    convert_action "${name%.*}"
  done
else
  names=""
  for source in *.gif *.mov *.webm *.mp4; do
    [[ -e "${source}" ]] || continue
    name="${source%.*}"
    case " ${names} " in
      *" ${name} "*) ;;
      *)
        names="${names} ${name}"
        convert_action "${name}"
        ;;
    esac
  done
fi
