uniform sampler2D s0;

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t0 : TEXCOORD0;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);

    r0.w = (input.t0.x * c0.x);
    r0.x = frac(r0.w);
    r0.x = (r0.w + -(r0.x));
    r0.x = (r0.x * c3.x);
    r0.z = (input.t0.y * c1.x);
    r0.w = frac(r0.z);
    r0.z = (r0.z + -(r0.w));
    r0.y = (r0.z * c2.x);
    r0 = tex2D(s0, r0.xy);
    r0.w = input.v0.w;
    output.oC0 = r0;
    return output;
}
