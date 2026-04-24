uniform sampler2D s0;

static const float4 c5 = float4(1.0, 0.5, 0.0, 0.0);

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t1 : TEXCOORD1;
    float4 t2 : TEXCOORD2;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);

    r0.w = (1.0 / input.t2.w);
    r0.x = (input.t2.x * r0.w + c5.x);
    r0.y = (input.t2.y * r0.w + c5.x);
    r0.x = (r0.x * c3.z);
    r1.w = c3.x;
    r0.z = (r0.x * c5.y + r1.w);
    r0.x = (r0.y * -(c5.y) + c5.x);
    r0.xy = (r0.zx * c3.ww + c3.yy);
    r0 = tex2D(s0, r0.xy);
    r1.xyz = (-(input.t1.xyz) + c2.xyz);
    r0.w = dot(r1.xyz, r1.xyz);
    r0.w = rsqrt(r0.w);
    r0.w = (1.0 / r0.w);
    r0.w = (r0.w + -(c4.y));
    r0.w = saturate((r0.w * c0.x));
    r1.xyz = lerp(input.v0.xyz, r0.xyz, r0.www);
    r0.xyz = (c1.xxx >= 0 ? r1.xyz : input.v0.xyz);
    r0.w = input.v0.w;
    output.oC0 = r0;
    return output;
}
