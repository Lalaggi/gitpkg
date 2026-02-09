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
  local cmds="install remove clean list upgrade update versions version goto help"
  if [ $COMP_CWORD -eq 1 ]; then
    COMPREPLY=( $(compgen -W "$cmds" -- "$cur") )
    return 0
  fi

  case "$cmd" in
    remove|clean|upgrade|update|versions|version|goto)
      pkgfile="${HOME}/.local/share/gitpkg/list.gitpkg"
      if [ -f "$pkgfile" ]; then
        packages=$(awk -F'=' '{gsub(/^ +| +$/,"",$1); print $1}' "$pkgfile" | tr '\n' ' ')
        COMPREPLY=( $(compgen -W "$packages" -- "$cur") )
      fi
      ;;
    install)
      COMPREPLY=()
      ;;
    goto)
      if [[ "$cur" == -* ]]; then
        COMPREPLY=( $(compgen -W "--shell -s" -- "$cur") )
      else
        pkgfile="${HOME}/.local/share/gitpkg/list.gitpkg"
        if [ -f "$pkgfile" ]; then
          packages=$(awk -F'=' '{gsub(/^ +| +$/,"",$1); print $1}' "$pkgfile" | tr '\n' ' ')
          COMPREPLY=( $(compgen -W "$packages" -- "$cur") )
        fi
      fi
      ;;
    *)
      COMPREPLY=()
      ;;
  esac
}

complete -F _gitpkg_completions gitpkg

