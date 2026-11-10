<!-- A person without a picture: one syllable of the given name on the person's colour. -->
<script lang="ts">
  import type { Person } from './data.ts';

  interface Props {
    person: Person;
    /** Next to a name that is already written out, the picture adds nothing for a screen reader. */
    decorative?: boolean;
    class?: string;
  }

  let { person, decorative = false, class: className = 'size-6' }: Props = $props();
</script>

<span
  role={decorative ? undefined : 'img'}
  aria-label={decorative ? undefined : person.name}
  aria-hidden={decorative ? 'true' : undefined}
  class={[
    'inline-flex shrink-0 items-center justify-center rounded-full text-xs font-medium text-foreground',
    className,
  ]}
  style:background-color="var(--chip-{person.hue})"
>
  {person.name.slice(1, 2)}
</span>
