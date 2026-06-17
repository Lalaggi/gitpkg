#!/usr/bin/env bash
#
# Bash completion for gitpkg
#
_gitpkg_completions() {
  local cur prev cmd pkgfile packages
  COMPREPLY=()
  cur="${COMP_WORDS[COMP_CWORD]}"
  prev="${COMP_WORDS[COMP_CWORD-1]}"
  cmd="${COMP_WORDS[1]:-}"

  # top-level commands
  local cmds="install remove clean list upgrade update versions version goto change-branch help"
  if [ $COMP_CWORD -eq 1 ]; then
    COMPREPLY=( $(compgen -W "$cmds" -- "$cur") )
    return 0
  fi

  # Complete --supplier/--provider/--host flag argument
  if [[ "$prev" == "--supplier" || "$prev" == "--provider" || "$prev" == "--host" ]]; then
    COMPREPLY=( $(compgen -W "gh github gl gitlab cb codeberg glg gnome gnome.gitlab gnome-gitlab gitlab.gnome gitlab-gnome" -- "$cur") )
    return 0
  fi

  # Read installed packages from list file
  pkgfile="${HOME}/.local/share/gitpkg/list.gitpkg"
  if [ -f "$pkgfile" ]; then
    packages=$(awk -F'=' '{gsub(/^ +| +$/,"",$1); print $1}' "$pkgfile" | tr '\n' ' ')
  fi

  case "$cmd" in
    install)
      COMPREPLY=( $(compgen -W "-v --supplier --branch" -- "$cur") )
      ;;
    remove|versions|version)
      COMPREPLY=( $(compgen -W "$packages self" -- "$cur") )
      ;;
    clean|upgrade|update)
      COMPREPLY=( $(compgen -W "all self $packages" -- "$cur") )
      ;;
    goto)
      if [[ "$cur" == -* ]]; then
        COMPREPLY=( $(compgen -W "--shell -s -v" -- "$cur") )
      else
        COMPREPLY=( $(compgen -W "$packages self" -- "$cur") )
      fi
      ;;
    change-branch)
      if [ $COMP_CWORD -eq 2 ]; then
        COMPREPLY=( $(compgen -W "$packages self" -- "$cur") )
      fi
      ;;
    *)
      COMPREPLY=()
      ;;
  esac
}

# Enable bash completion emulation in zsh if available
if [[ -n "${ZSH_VERSION-}" ]]; then
  autoload -Uz bashcompinit 2>/dev/null && bashcompinit 2>/dev/null
fi

complete -F _gitpkg_completions gitpkg
